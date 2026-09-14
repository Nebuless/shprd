use crate::{
    ConnectionId, Error, LEGACY_ID, Manager, Profile, Registry, Result, RuntimeFactory, State,
    Store, error::invalid,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
pub type ProfileFactory = Arc<dyn Fn(&Profile) -> RuntimeFactory + Send + Sync>;
/// Host serializes mutations through exclusive access. Manager leases remain concurrent.
pub struct ProfileService {
    pub(crate) store: Store,
    pub(crate) registry: Option<Registry>,
    pub(crate) profiles: BTreeMap<String, Profile>,
    pub(crate) manager: Arc<Manager>,
    pub(crate) factory: ProfileFactory,
    pub(crate) explicit_legacy: bool,
    pub(crate) mutation_error: Option<String>,
}
impl ProfileService {
    pub fn load(
        store: Store,
        legacy: Profile,
        explicit_legacy: bool,
        manager: Arc<Manager>,
        factory: ProfileFactory,
    ) -> Result<Self> {
        if legacy.id().as_str() != LEGACY_ID {
            return Err(invalid("bootstrap requires a legacy profile"));
        }
        let (registry, mutation_error) = match store.load() {
            Ok(registry) => (registry, None),
            Err(e) => (
                None,
                Some(format!(
                    "connection registry is invalid; repair before changing profiles: {e}"
                )),
            ),
        };
        let mut profiles = BTreeMap::new();
        if registry.is_none() || explicit_legacy {
            profiles.insert(LEGACY_ID.into(), legacy);
        }
        if let Some(r) = &registry {
            for p in &r.profiles {
                profiles.insert(p.id().as_str().into(), p.clone());
            }
        }
        for p in profiles.values() {
            manager.register(p.clone(), factory(p))?;
        }
        let default = match &registry {
            Some(r) if !explicit_legacy => r.default_connection_id.clone(),
            Some(_) | None => ConnectionId::parse(LEGACY_ID)?,
        };
        manager.set_default(&default)?;
        Ok(Self {
            store,
            registry,
            profiles,
            manager,
            factory,
            explicit_legacy,
            mutation_error,
        })
    }
    pub fn mutation_error(&self) -> Option<&str> {
        self.mutation_error.as_deref()
    }
    pub(crate) fn writable(&self, id: &ConnectionId) -> Result<&Profile> {
        self.can_mutate()?;
        if id.as_str() == LEGACY_ID {
            return Err(invalid("connection profile is read-only"));
        }
        self.profile(id)
    }
    pub(crate) fn can_mutate(&self) -> Result<()> {
        match &self.mutation_error {
            Some(e) => Err(invalid(e)),
            None => Ok(()),
        }
    }
    pub fn profile(&self, id: &ConnectionId) -> Result<&Profile> {
        self.profiles
            .get(id.as_str())
            .ok_or_else(|| invalid("unknown connection"))
    }
    pub fn item(&self, id: &ConnectionId) -> Result<Value> {
        let p = self.profile(id)?;
        let mut item = serde_json::to_value(p)?;
        let object = item
            .as_object_mut()
            .ok_or_else(|| invalid("profile is not an object"))?;
        if let Value::Object(status) = serde_json::to_value(self.manager.status(id)?)? {
            object.extend(status);
        }
        object.insert("read_only".into(), json!(id.as_str() == LEGACY_ID));
        Ok(item)
    }
    pub fn list(&self) -> Result<Vec<Value>> {
        self.profiles.values().map(|p| self.item(p.id())).collect()
    }
    pub async fn start_configured(&self) -> Vec<(ConnectionId, Result<()>)> {
        let default = self.manager.default_id().ok();
        let mut results = Vec::new();
        for p in self
            .profiles
            .values()
            .filter(|p| p.auto_connect() || Some(p.id()) == default.as_ref())
        {
            results.push((p.id().clone(), self.manager.connect(p.id()).await));
        }
        results
    }
    pub async fn connect(&self, id: &ConnectionId) -> Result<Value> {
        self.profile(id)?;
        self.manager.connect(id).await?;
        self.item(id)
    }
    pub async fn disconnect(&self, id: &ConnectionId) -> Result<Value> {
        self.profile(id)?;
        self.manager.disconnect(id).await?;
        self.item(id)
    }
    pub(crate) fn rollback_store(&mut self) -> Result<()> {
        let result = match &self.registry {
            Some(r) => self.store.save(r),
            None => self.store.clear(),
        };
        if result.is_err() {
            self.mutation_error = Some(
                "connection profile mutations disabled because persistence rollback failed".into(),
            );
        }
        result
    }
    pub fn set_default(&mut self, id: &ConnectionId) -> Result<Value> {
        self.writable(id)?;
        if self.explicit_legacy {
            return Err(invalid(
                "explicit CLI/environment connection remains the process default",
            ));
        }
        let mut next = self
            .registry
            .clone()
            .ok_or_else(|| invalid("no persisted connection registry"))?;
        next.version = 2;
        next.default_connection_id = id.clone();
        self.store.save(&next)?;
        if let Err(error) = self.manager.set_default(id) {
            self.rollback_store()?;
            return Err(error);
        }
        self.registry = Some(next);
        Ok(json!({"ok":true,"default_connection_id":id}))
    }
    pub async fn remove(&mut self, id: &ConnectionId) -> Result<Value> {
        let old = self.writable(id)?.clone();
        if self.manager.default_id()? == *id {
            return Err(invalid("cannot remove the default connection"));
        }
        let mut next = self
            .registry
            .clone()
            .ok_or_else(|| invalid("no persisted connection registry"))?;
        next.version = 2;
        next.profiles.retain(|p| p.id() != id);
        if next.profiles.is_empty() && !self.explicit_legacy {
            return Err(invalid("cannot remove the last persisted connection"));
        }
        let connected = matches!(
            self.manager.status(id)?.state,
            State::Ready | State::Connecting | State::Reconnecting
        );
        self.manager.unregister(id).await?;
        let save = if next.profiles.is_empty() {
            self.store.clear()
        } else {
            self.store.save(&next)
        };
        if let Err(error) = save {
            self.manager.register(old.clone(), (self.factory)(&old))?;
            if connected {
                self.manager.connect(id).await?;
            }
            return Err(error);
        }
        self.profiles.remove(id.as_str());
        self.registry = if next.profiles.is_empty() {
            None
        } else {
            Some(next)
        };
        Ok(json!({"ok":true}))
    }
    /// Probe callback must validate control ping and render protocol, without altering live runtimes.
    pub async fn test<F, Fut>(&self, profile: &Profile, probe: F) -> Result<ProbeResult>
    where
        F: FnOnce(Profile) -> Fut,
        Fut: std::future::Future<Output = Result<ProbeResult>>,
    {
        let result = probe(profile.clone()).await?;
        result.validate()?;
        Ok(result)
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProbeResult {
    pub ok: bool,
    pub version: Option<String>,
    pub protocol: u32,
}
impl ProbeResult {
    pub fn validate(&self) -> Result<()> {
        if !self.ok || !matches!(self.protocol, 14..=20 | 22) {
            return Err(Error::Runtime {
                message: "Herdr protocol is not supported".into(),
                retryable: false,
            });
        }
        Ok(())
    }
}
