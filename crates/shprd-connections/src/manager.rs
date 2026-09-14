use crate::{ConnectionId, Error, Profile, Result, RetryPolicy, error::invalid, runtime::*};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
#[cfg(test)]
use tokio::sync::Notify;

pub(crate) struct Entries {
    entries: BTreeMap<String, Arc<Entry>>,
    default: ConnectionId,
    pub(crate) stopping: bool,
}
#[derive(Clone)]
pub struct Manager {
    pub(crate) inner: Arc<Mutex<Entries>>,
}
impl Manager {
    pub fn new(default: ConnectionId) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Entries {
                entries: BTreeMap::new(),
                default,
                stopping: false,
            })),
        }
    }
    pub fn default_id(&self) -> Result<ConnectionId> {
        Ok(lock(&self.inner)?.default.clone())
    }
    pub(crate) fn entry(&self, id: &ConnectionId) -> Result<Arc<Entry>> {
        lock(&self.inner)?
            .entries
            .get(id.as_str())
            .cloned()
            .ok_or_else(|| Error::Routing {
                status: 404,
                message: format!("unknown connection: {}", id.as_str()),
            })
    }
    pub fn register(&self, profile: Profile, factory: RuntimeFactory) -> Result<()> {
        let mut inner = lock(&self.inner)?;
        if inner.stopping {
            return Err(invalid("connection manager is stopping"));
        }
        if inner.entries.contains_key(profile.id().as_str()) {
            return Err(invalid("connection already registered"));
        }
        let (cancel, _) = tokio::sync::watch::channel(0);
        inner.entries.insert(
            profile.id().as_str().to_owned(),
            Arc::new(Entry {
                data: Mutex::new(EntryData {
                    profile,
                    generation: 0,
                    disconnect_revision: 0,
                    state: State::Disconnected,
                    error: None,
                    runtime: None,
                    paths: None,
                    factory,
                    retry: RetryPolicy::default(),
                    registered: true,
                }),
                operation: tokio::sync::Mutex::new(()),
                cancel,
                retry_task: Mutex::new(None),
                #[cfg(test)]
                retry_task_observation: Arc::new(RetryTaskObservation {
                    armed: Notify::new(),
                    completed: Notify::new(),
                    ready: Notify::new(),
                    completion_count: std::sync::atomic::AtomicUsize::new(0),
                }),
            }),
        );
        Ok(())
    }
    pub fn status(&self, id: &ConnectionId) -> Result<Status> {
        let entry = self.entry(id)?;
        let default = self.default_id()?;
        let d = lock(&entry.data)?;
        Ok(Status {
            id: d.profile.id().clone(),
            label: d.profile.label().into(),
            source: d.profile.source().into(),
            is_default: *id == default,
            state: d.state,
            generation: d.generation,
            error: d.error.clone(),
        })
    }
    pub fn list(&self) -> Result<Vec<Status>> {
        let ids: Vec<_> = lock(&self.inner)?
            .entries
            .keys()
            .map(|id| ConnectionId::parse(id))
            .collect::<Result<_>>()?;
        ids.iter().map(|id| self.status(id)).collect()
    }
    pub fn set_default(&self, id: &ConnectionId) -> Result<()> {
        let mut inner = lock(&self.inner)?;
        if inner.stopping {
            return Err(invalid("connection manager is stopping"));
        }
        if !inner.entries.contains_key(id.as_str()) {
            return Err(invalid("unknown connection"));
        }
        inner.default = id.clone();
        Ok(())
    }
    pub fn lease(&self, id: &ConnectionId) -> Result<Lease> {
        let entry = self.entry(id)?;
        let d = lock(&entry.data)?;
        if d.state != State::Ready || !d.registered {
            return Err(Error::Routing {
                status: 503,
                message: format!("connection is not ready: {}", id.as_str()),
            });
        }
        Ok(Lease {
            connection_id: id.clone(),
            paths: d
                .paths
                .clone()
                .ok_or_else(|| invalid("ready connection has no socket paths"))?,
            context: RuntimeContext {
                entry: Arc::downgrade(&entry),
                manager: Arc::downgrade(&self.inner),
                generation: d.generation,
            },
        })
    }
    pub fn resolve(&self, id: Option<&ConnectionId>, generation: Option<u64>) -> Result<Lease> {
        let default = self.default_id()?;
        let lease = self.lease(id.unwrap_or(&default))?;
        if generation.is_some_and(|g| g != lease.generation()) {
            return Err(Error::Routing {
                status: 409,
                message: format!(
                    "connection generation changed: {}",
                    lease.connection_id.as_str()
                ),
            });
        }
        Ok(lease)
    }
    pub(crate) fn schedule_retry(&self, context: RuntimeContext) -> Result<()> {
        let entry = context.entry.upgrade().ok_or(Error::Stale)?;
        let id = {
            let mut d = lock(&entry.data)?;
            if !d.registered || d.generation != context.generation || d.state != State::Reconnecting
            {
                return Ok(());
            }
            d.retry.enable_retry();
            d.profile.id().clone()
        };
        if let Some(task) = lock(&entry.retry_task)?.take() {
            task.abort();
        }
        let manager = self.clone();
        let retry_entry = Arc::clone(&entry);
        #[cfg(test)]
        let retry_task_observation = Arc::clone(&entry.retry_task_observation);
        let task = tokio::spawn(async move {
            #[cfg(test)]
            let _completion = RetryTaskCompletion {
                observation: Arc::clone(&retry_task_observation),
            };
            for _ in 0..6 {
                let ticket = {
                    let mut d = match lock(&retry_entry.data) {
                        Ok(d) => d,
                        Err(_) => return,
                    };
                    d.retry.schedule(true, 0.5)
                };
                let Some(ticket) = ticket else { return };
                let mut cancel = retry_entry.cancel.subscribe();
                #[cfg(test)]
                retry_task_observation.armed.notify_one();
                tokio::select! {
                    _ = tokio::time::sleep(ticket.delay) => {}
                    _ = cancel.changed() => return,
                }
                let current = match lock(&retry_entry.data) {
                    Ok(d) => {
                        d.registered
                            && d.retry.is_current(&ticket)
                            && d.state == State::Reconnecting
                    }
                    Err(_) => false,
                };
                if !current {
                    return;
                }
                let generation = match lock(&retry_entry.data) {
                    Ok(d) => d.generation,
                    Err(_) => return,
                };
                match manager
                    .connect_if_current(&id, None, Some(generation))
                    .await
                {
                    Ok(()) => return,
                    Err(Error::Runtime {
                        retryable: false, ..
                    }) => return,
                    Err(_) => {}
                }
                let should_continue = match lock(&retry_entry.data) {
                    Ok(mut d) => {
                        if d.registered && d.state == State::Error {
                            d.state = State::Reconnecting;
                            true
                        } else {
                            false
                        }
                    }
                    Err(_) => false,
                };
                if !should_continue {
                    return;
                }
            }
        });
        *lock(&entry.retry_task)? = Some(task);
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn retry_task_observation(
        &self,
        id: &ConnectionId,
    ) -> Result<Arc<RetryTaskObservation>> {
        let entry = self.entry(id)?;
        Ok(Arc::clone(&entry.retry_task_observation))
    }
    pub async fn replace(&self, profile: Profile, factory: RuntimeFactory) -> Result<()> {
        let entry = self.entry(profile.id())?;
        {
            let mut d = lock(&entry.data)?;
            d.profile = profile.clone();
            d.factory = factory;
        }
        self.retire(profile.id(), false).await
    }
    pub async fn unregister(&self, id: &ConnectionId) -> Result<()> {
        if *id == self.default_id()? {
            return Err(invalid("cannot remove the default connection"));
        }
        let entry = self.entry(id)?;
        let cleanup = self.disconnect(id).await;
        if cleanup.is_ok() {
            lock(&entry.data)?.registered = false;
            lock(&self.inner)?.entries.remove(id.as_str());
        }
        cleanup
    }
    pub async fn stop_all(&self) -> Result<()> {
        let ids = {
            let mut inner = lock(&self.inner)?;
            inner.stopping = true;
            inner
                .entries
                .keys()
                .map(|id| ConnectionId::parse(id))
                .collect::<Result<Vec<_>>>()?
        };
        for id in &ids {
            let entry = self.entry(id)?;
            let mut d = lock(&entry.data)?;
            advance(&mut d)?;
            d.disconnect_revision = d.generation;
            d.state = State::Stopping;
            d.paths = None;
            entry.cancel.send_replace(d.generation);
        }
        let mut failure = None;
        for id in ids {
            if let Err(e) = self.disconnect(&id).await {
                failure = Some(e);
            }
        }
        match failure {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
struct RetryTaskCompletion {
    observation: Arc<RetryTaskObservation>,
}

#[cfg(test)]
impl Drop for RetryTaskCompletion {
    fn drop(&mut self) {
        self.observation
            .completion_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.observation.completed.notify_one();
    }
}
