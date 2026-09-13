use crate::{
    ConnectionId, Error, ProbeResult, Profile, ProfileService, Result, State, error::invalid,
    runtime::lock,
};
impl ProfileService {
    pub async fn update<F, Fut>(
        &mut self,
        id: &ConnectionId,
        replacement: Profile,
        probe: F,
    ) -> Result<serde_json::Value>
    where
        F: FnOnce(Profile) -> Fut,
        Fut: std::future::Future<Output = Result<ProbeResult>>,
    {
        let old = self.writable(id)?.clone();
        if replacement.id() != id {
            return Err(invalid("connection profile id cannot be changed"));
        }
        let entry = self.manager.entry(id)?;
        let (state, disconnect_revision) = {
            let data = lock(&entry.data)?;
            (data.state, data.disconnect_revision)
        };
        let connected = matches!(
            state,
            State::Ready | State::Connecting | State::Reconnecting
        );
        if state == State::Ready {
            self.test(&replacement, probe).await?;
        }
        if lock(&entry.data)?.disconnect_revision != disconnect_revision {
            return Err(Error::Stale);
        }
        let previous = self
            .registry
            .clone()
            .ok_or_else(|| invalid("no persisted connection registry"))?;
        let mut next = previous.clone();
        next.version = 2;
        for p in &mut next.profiles {
            if p.id() == id {
                *p = replacement.clone();
            }
        }
        self.store.save(&next)?;
        let apply = async {
            self.manager
                .replace(replacement.clone(), (self.factory)(&replacement))
                .await?;
            if connected || replacement.auto_connect() || self.manager.default_id()? == *id {
                self.manager
                    .connect_if_current(id, Some(disconnect_revision))
                    .await?;
            }
            Ok::<_, crate::Error>(())
        }
        .await;
        if let Err(error) = apply {
            if let Err(rollback) = self.store.save(&previous) {
                self.mutation_error = Some(
                    "connection profile mutations disabled because persistence rollback failed"
                        .into(),
                );
                self.registry = Some(next);
                self.profiles.insert(id.as_str().into(), replacement);
                let _cleanup = self.manager.disconnect(id).await;
                return Err(invalid(format!(
                    "connection update failed: {error}; rollback incomplete: {rollback}"
                )));
            }
            self.manager
                .replace(old.clone(), (self.factory)(&old))
                .await?;
            if connected && lock(&entry.data)?.disconnect_revision == disconnect_revision {
                self.manager
                    .connect_if_current(id, Some(disconnect_revision))
                    .await?;
            }
            return Err(error);
        }
        self.registry = Some(next);
        self.profiles.insert(id.as_str().into(), replacement);
        self.item(id)
    }
}
