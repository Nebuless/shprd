use crate::{ConnectionId, Error, Manager, Result, error::invalid, runtime::*};
use std::sync::Arc;
impl Manager {
    pub async fn connect(&self, id: &ConnectionId) -> Result<()> {
        self.connect_if_current_with_reset(id, None, None, true)
            .await
    }
    pub(crate) async fn connect_if_current(
        &self,
        id: &ConnectionId,
        disconnect_revision: Option<u64>,
        expected_generation: Option<u64>,
    ) -> Result<()> {
        self.connect_if_current_with_reset(id, disconnect_revision, expected_generation, false)
            .await
    }
    async fn connect_if_current_with_reset(
        &self,
        id: &ConnectionId,
        disconnect_revision: Option<u64>,
        expected_generation: Option<u64>,
        explicit: bool,
    ) -> Result<()> {
        if lock(&self.inner)?.stopping {
            return Err(invalid("connection manager is stopping"));
        }
        let entry = self.entry(id)?;
        let _operation = entry.operation.lock().await;
        if lock(&self.inner)?.stopping {
            return Err(invalid("connection manager is stopping"));
        }
        let (retired, before_retirement) = {
            let mut d = lock(&entry.data)?;
            if !d.registered
                || disconnect_revision.is_some_and(|revision| revision != d.disconnect_revision)
                || expected_generation.is_some_and(|generation| generation != d.generation)
            {
                return Err(Error::Stale);
            }
            if d.state == State::Ready {
                return Ok(());
            }
            if explicit {
                if let Some(task) = lock(&entry.retry_task)?.take() {
                    task.abort();
                }
                d.retry.enable();
            }
            (d.runtime.take(), d.generation)
        };
        if let Some(runtime) = retired
            && let Err(error) = runtime.stop().await
        {
            let mut data = lock(&entry.data)?;
            data.runtime = Some(runtime);
            data.state = State::Error;
            data.error = Some(StatusError {
                message: sanitize_error(&error.to_string()),
            });
            return Err(error);
        }
        let (context, factory) = {
            let mut d = lock(&entry.data)?;
            if !d.registered
                || d.generation != before_retirement
                || expected_generation.is_some_and(|generation| generation != d.generation)
                || disconnect_revision.is_some_and(|revision| revision != d.disconnect_revision)
            {
                return Err(Error::Stale);
            }
            advance(&mut d)?;
            d.state = State::Connecting;
            d.error = None;
            (
                RuntimeContext {
                    entry: Arc::downgrade(&entry),
                    manager: Arc::downgrade(&self.inner),
                    generation: d.generation,
                },
                d.factory.clone(),
            )
        };
        let runtime = match factory(context.clone()) {
            Ok(runtime) => runtime,
            Err(error) => {
                context.report_error(&error.to_string(), false)?;
                return Err(error);
            }
        };
        lock(&entry.data)?.runtime = Some(runtime.clone());
        let result = tokio::select! { biased; _ = context.cancelled() => Err(Error::Stale), result = runtime.start(&context) => result };
        match result {
            Ok(paths) => {
                let accepted = {
                    let mut d = lock(&entry.data)?;
                    if d.registered && d.generation == context.generation {
                        d.paths = Some(paths);
                        d.state = State::Ready;
                        d.retry.mark_ready();
                        true
                    } else {
                        false
                    }
                };
                if accepted {
                    #[cfg(test)]
                    entry.retry_task_observation.ready.notify_one();
                    return Ok(());
                }
                let cleanup = runtime.stop().await;
                let mut data = lock(&entry.data)?;
                if cleanup.is_err() {
                    data.runtime = Some(runtime);
                } else if data
                    .runtime
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &runtime))
                {
                    data.runtime = None;
                }
                drop(data);
                cleanup?;
                Err(Error::Stale)
            }
            Err(error) => {
                context.report_error(&error.to_string(), false)?;
                let cleanup = runtime.stop().await;
                let mut d = lock(&entry.data)?;
                if cleanup.is_err() {
                    d.runtime = Some(runtime);
                } else if d
                    .runtime
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &runtime))
                {
                    d.runtime = None;
                }
                drop(d);
                cleanup?;
                Err(error)
            }
        }
    }
    /// Invalidates before awaiting startup or process cleanup.
    pub async fn disconnect(&self, id: &ConnectionId) -> Result<()> {
        self.retire(id, true).await
    }
    pub(crate) async fn retire(&self, id: &ConnectionId, explicit: bool) -> Result<()> {
        let entry = self.entry(id)?;
        {
            let mut d = lock(&entry.data)?;
            advance(&mut d)?;
            if explicit {
                d.disconnect_revision = d.generation;
            }
            d.retry.disable();
            if let Some(task) = lock(&entry.retry_task)?.take() {
                task.abort();
            }
            d.state = State::Stopping;
            d.paths = None;
            d.error = None;
            entry.cancel.send_replace(d.generation);
        }
        let _operation = entry.operation.lock().await;
        let runtime = lock(&entry.data)?.runtime.take();
        let result = match runtime.as_ref() {
            Some(runtime) => runtime.stop().await,
            None => Ok(()),
        };
        let mut d = lock(&entry.data)?;
        match &result {
            Ok(()) => {
                d.state = State::Disconnected;
                d.error = None;
            }
            Err(e) => {
                d.runtime = runtime;
                d.state = State::Error;
                d.error = Some(StatusError {
                    message: sanitize_error(&e.to_string()),
                });
            }
        }
        result
    }
}
