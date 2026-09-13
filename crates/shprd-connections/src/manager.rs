use crate::{ConnectionId, Error, Profile, Result, error::invalid, runtime::*};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub(crate) struct Entries {
    entries: BTreeMap<String, Arc<Entry>>,
    default: ConnectionId,
    pub(crate) stopping: bool,
}
pub struct Manager {
    pub(crate) inner: Mutex<Entries>,
}
impl Manager {
    pub fn new(default: ConnectionId) -> Self {
        Self {
            inner: Mutex::new(Entries {
                entries: BTreeMap::new(),
                default,
                stopping: false,
            }),
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
                    registered: true,
                }),
                operation: tokio::sync::Mutex::new(()),
                cancel,
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
        {
            let mut d = lock(&entry.data)?;
            d.registered = false;
        }
        let cleanup = self.disconnect(id).await;
        lock(&self.inner)?.entries.remove(id.as_str());
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
