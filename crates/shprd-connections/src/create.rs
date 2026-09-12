use crate::{ConnectionId, LEGACY_ID, Profile, ProfileService, Registry, Result, error::invalid};
use serde_json::Value;
impl ProfileService {
    pub async fn create(&mut self, profile: Profile) -> Result<Value> {
        self.can_mutate()?;
        if profile.id().as_str() == LEGACY_ID || self.profiles.contains_key(profile.id().as_str()) {
            return Err(invalid("connection already exists or is reserved"));
        }
        let migration = self.registry.is_none() && !self.explicit_legacy;
        let seed = if migration {
            self.profiles
                .get(LEGACY_ID)
                .map(|p| p.migration_seed(profile.id().as_str()))
                .transpose()?
        } else {
            None
        };
        let mut next = self.registry.clone().unwrap_or(Registry {
            version: 2,
            default_connection_id: profile.id().clone(),
            profiles: Vec::new(),
        });
        next.version = 2;
        if let Some(seed) = &seed {
            next.profiles.push(seed.clone());
        }
        next.profiles.push(profile.clone());
        self.store.save(&next)?;
        let registration = (|| {
            if let Some(seed) = &seed {
                self.manager.register(seed.clone(), (self.factory)(seed))?;
            }
            self.manager
                .register(profile.clone(), (self.factory)(&profile))
        })();
        if let Err(error) = registration {
            if let Some(seed) = &seed
                && self.manager.status(seed.id()).is_ok()
            {
                let _cleanup = self.manager.unregister(seed.id()).await;
            }
            self.rollback_store()?;
            return Err(error);
        }
        self.registry = Some(next);
        if let Some(seed) = &seed {
            self.profiles
                .insert(seed.id().as_str().into(), seed.clone());
        }
        self.profiles
            .insert(profile.id().as_str().into(), profile.clone());
        if migration {
            self.manager.set_default(profile.id())?;
            let _cleanup = self
                .manager
                .unregister(&ConnectionId::parse(LEGACY_ID)?)
                .await;
            self.profiles.remove(LEGACY_ID);
        }
        if let Some(seed) = &seed {
            let _startup = self.manager.connect(seed.id()).await;
        }
        if profile.auto_connect() || self.manager.default_id()? == *profile.id() {
            let _startup = self.manager.connect(profile.id()).await;
        }
        self.item(profile.id())
    }
}
