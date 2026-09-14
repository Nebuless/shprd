//! Authentication cookies compatible with the existing React host.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const TTL_SECONDS: u64 = 30 * 24 * 60 * 60;
type Signature = Hmac<Sha256>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("authentication requires a non-empty signing secret")]
    EmptySecret,
    #[error("system clock precedes Unix epoch")]
    Clock,
    #[error("secure random source unavailable: {0}")]
    Random(String),
    #[error("invalid signing secret")]
    Key,
    #[error("authentication token file: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid generated auth token file")]
    Token,
}

#[derive(Clone)]
pub struct Auth {
    required: bool,
    secret: String,
    cookie_name: &'static str,
}

impl Auth {
    pub const fn required(&self) -> bool {
        self.required
    }

    pub fn new(required: bool, secret: String) -> Result<Self, Error> {
        Self::new_with_cookie(required, secret, "herdr_auth")
    }

    pub fn new_with_cookie(
        required: bool,
        secret: String,
        cookie_name: &'static str,
    ) -> Result<Self, Error> {
        if required && secret.is_empty() {
            return Err(Error::EmptySecret);
        }
        Ok(Self {
            required,
            secret,
            cookie_name,
        })
    }

    fn signer(&self) -> Result<Signature, Error> {
        Signature::new_from_slice(self.secret.as_bytes()).map_err(|_| Error::Key)
    }

    pub fn login(&self, password: &str) -> Result<Option<String>, Error> {
        let mut expected = self.signer()?;
        expected.update(self.secret.as_bytes());
        let mut supplied = self.signer()?;
        supplied.update(password.as_bytes());
        if expected
            .verify_slice(&supplied.finalize().into_bytes())
            .is_err()
        {
            return Ok(None);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Clock)?
            .as_secs();
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce).map_err(|e| Error::Random(e.to_string()))?;
        let payload = URL_SAFE_NO_PAD
            .encode(json!({"iat":now,"exp":now+TTL_SECONDS,"nonce":hex(&nonce)}).to_string());
        let mut signer = self.signer()?;
        signer.update(payload.as_bytes());
        Ok(Some(format!(
            "{}={payload}.{}; HttpOnly; SameSite=Lax; Path=/; Max-Age={TTL_SECONDS}",
            self.cookie_name,
            hex(&signer.finalize().into_bytes())
        )))
    }

    pub fn authenticated(&self, cookie: Option<&str>) -> bool {
        if !self.required {
            return true;
        }
        let Some(token) = cookie.and_then(|header| {
            header
                .split(';')
                .find_map(|part| part.trim().strip_prefix(&format!("{}=", self.cookie_name)))
        }) else {
            return false;
        };
        let Some((payload, signature)) = token.split_once('.') else {
            return false;
        };
        let Some(signature) = decode_hex(signature) else {
            return false;
        };
        let Ok(mut signer) = self.signer() else {
            return false;
        };
        signer.update(payload.as_bytes());
        if signer.verify_slice(&signature).is_err() {
            return false;
        }
        let Ok(bytes) = URL_SAFE_NO_PAD.decode(payload) else {
            return false;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return false;
        };
        let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
            return false;
        };
        value
            .get("exp")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|expiry| expiry > now.as_secs())
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() != 64 || !value.is_ascii() {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).ok())
        .collect()
}

pub fn load_or_create_token(path: &Path) -> Result<String, Error> {
    let parent = path.parent().ok_or(Error::Token)?;
    let mut directory = fs::DirBuilder::new();
    directory.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(parent)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            let mut bytes = [0_u8; 32];
            getrandom::fill(&mut bytes).map_err(|e| Error::Random(e.to_string()))?;
            let token = hex(&bytes);
            writeln!(file, "{token}")?;
            file.sync_all()?;
            Ok(token)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.file_type().is_file() {
                return Err(Error::Token);
            }
            let file = fs::File::open(path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            let mut contents = String::new();
            file.take(129).read_to_string(&mut contents)?;
            let token = contents.trim();
            if contents.len() > 128
                || token.len() != 64
                || !token
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Error::Token);
            }
            Ok(token.to_owned())
        }
        Err(error) => Err(error.into()),
    }
}
