use crate::{Result, error::invalid};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const LEGACY_ID: &str = "legacy-default";
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct ConnectionId(String);
impl ConnectionId {
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty()
            || value.len() > 128
            || !value.as_bytes()[0].is_ascii_alphanumeric()
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        {
            return Err(invalid("invalid connection_id"));
        }
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for ConnectionId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Transport {
    Local {
        control_socket_path: String,
        client_socket_path: String,
    },
    Ssh {
        ssh_destination: String,
        remote_control_socket_path: String,
        remote_client_socket_path: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Profile {
    id: ConnectionId,
    label: String,
    #[serde(flatten)]
    transport: Transport,
    auto_connect: bool,
}
impl Profile {
    pub fn parse(raw: &str) -> Result<Self> {
        Self::from_value(serde_json::from_str(raw)?)
    }
    pub fn from_value(value: serde_json::Value) -> Result<Self> {
        let mut object = value
            .as_object()
            .cloned()
            .ok_or_else(|| invalid("connection profile must be an object"))?;
        let id: ConnectionId =
            serde_json::from_value(object.remove("id").ok_or_else(|| invalid("missing id"))?)?;
        let label: String = serde_json::from_value(
            object
                .remove("label")
                .ok_or_else(|| invalid("missing label"))?,
        )?;
        let auto_connect: bool = serde_json::from_value(
            object
                .remove("auto_connect")
                .ok_or_else(|| invalid("missing auto_connect"))?,
        )?;
        let transport: Transport = serde_json::from_value(object.into())?;
        if id.as_str() == LEGACY_ID {
            return Err(invalid("legacy-default is reserved"));
        }
        if label.is_empty()
            || label.encode_utf16().count() > 80
            || label.trim() != label
            || label.chars().any(char::is_control)
        {
            return Err(invalid("connection label is invalid"));
        }
        let transport = match transport {
            Transport::Local {
                control_socket_path,
                client_socket_path,
            } => Transport::Local {
                control_socket_path: local_path(&control_socket_path)?,
                client_socket_path: local_path(&client_socket_path)?,
            },
            Transport::Ssh {
                ssh_destination,
                remote_control_socket_path,
                remote_client_socket_path,
            } => {
                validate_destination(&ssh_destination)?;
                validate_remote_path(&remote_control_socket_path)?;
                validate_remote_path(&remote_client_socket_path)?;
                if !remote_control_socket_path.is_empty()
                    && remote_control_socket_path == remote_client_socket_path
                {
                    return Err(invalid(
                        "remote control and render socket paths must differ",
                    ));
                }
                Transport::Ssh {
                    ssh_destination,
                    remote_control_socket_path,
                    remote_client_socket_path,
                }
            }
        };
        Ok(Self {
            id,
            label,
            transport,
            auto_connect,
        })
    }
    pub fn id(&self) -> &ConnectionId {
        &self.id
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub const fn transport(&self) -> &Transport {
        &self.transport
    }
    pub const fn auto_connect(&self) -> bool {
        self.auto_connect
    }
    pub fn legacy(control: &str, render: &str) -> Result<Self> {
        Ok(Self {
            id: ConnectionId::parse(LEGACY_ID)?,
            label: "Default".into(),
            transport: Transport::Local {
                control_socket_path: local_path(control)?,
                client_socket_path: local_path(render)?,
            },
            auto_connect: true,
        })
    }
    pub(crate) fn migration_seed(&self, new_id: &str) -> Result<Self> {
        Ok(Self {
            id: ConnectionId::parse(if new_id == "local" {
                "localhost"
            } else {
                "local"
            })?,
            label: "Local".into(),
            transport: self.transport.clone(),
            auto_connect: true,
        })
    }
    pub fn source(&self) -> &str {
        if self.id.as_str() == LEGACY_ID {
            return "legacy-config";
        }
        match self.transport {
            Transport::Local { .. } => "local-profile",
            Transport::Ssh { .. } => "ssh-profile",
        }
    }
}
impl<'de> Deserialize<'de> for Profile {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::from_value(serde_json::Value::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

use crate::paths::{local_path, validate_destination, validate_remote_path};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub version: u8,
    pub default_connection_id: ConnectionId,
    pub profiles: Vec<Profile>,
}
impl Registry {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() > 1024 * 1024 {
            return Err(invalid("connection registry is too large"));
        }
        let registry: Self = serde_json::from_slice(raw)?;
        registry.validate()?;
        Ok(registry)
    }
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.version, 1 | 2) {
            return Err(invalid("unsupported connection registry version"));
        }
        if self.profiles.is_empty() || self.profiles.len() > 64 {
            return Err(invalid("connection registry must contain 1 to 64 profiles"));
        }
        let mut ids = HashSet::new();
        for p in &self.profiles {
            if p.id.as_str() == LEGACY_ID {
                return Err(invalid("legacy-default is reserved"));
            }
            if self.version == 1 && matches!(p.transport, Transport::Ssh { .. }) {
                return Err(invalid("version 1 supports local profiles only"));
            }
            if !ids.insert(&p.id) {
                return Err(invalid("duplicate connection profile"));
            }
        }
        if !ids.contains(&self.default_connection_id) {
            return Err(invalid("default connection profile does not exist"));
        }
        Ok(())
    }
}
