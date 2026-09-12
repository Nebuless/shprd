//! Validated host configuration and shell bridge wire contracts.

use serde::Deserialize;
use url::Url;

/// Validated HTTP(S) origin serving the retained React client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostUrl(Url);

/// Invalid host input; never includes the potentially secret input value.
#[derive(Debug, thiserror::Error)]
#[error("Enter an HTTP(S) origin without credentials, path, query or fragment.")]
pub struct InvalidHost;

impl HostUrl {
    /// Parse a host origin without accepting credentials or application routes.
    pub fn parse(input: &str) -> Result<Self, InvalidHost> {
        if input.chars().any(char::is_whitespace) {
            return Err(InvalidHost);
        }
        let url = Url::parse(input).map_err(|_| InvalidHost)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(InvalidHost);
        }
        Ok(Self(url))
    }

    /// Canonical root URL used by the iframe.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Exact serialized origin used by postMessage.
    pub fn origin(&self) -> String {
        self.0.origin().ascii_serialization()
    }
}

#[derive(Debug, Deserialize)]
enum Protocol {
    #[serde(rename = "shprd.shell.v1")]
    V1,
}

#[derive(Debug, Deserialize)]
enum AckType {
    #[serde(rename = "ack")]
    Ack,
}

/// Acknowledgement from the owning React window after browser provenance checks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeAck {
    protocol: Protocol,
    #[serde(rename = "type")]
    kind: AckType,
    request_id: String,
}

impl BridgeAck {
    /// Match only the current outstanding request.
    pub fn accepts(&self, request_id: &str) -> bool {
        matches!((&self.protocol, &self.kind), (Protocol::V1, AckType::Ack))
            && self.request_id == request_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_normalizes_https_origin_when_valid() -> Result<(), Box<dyn std::error::Error>> {
        let host = HostUrl::parse("https://example.com:443")?;
        assert_eq!(host.as_str(), "https://example.com/");
        assert_eq!(host.origin(), "https://example.com");
        Ok(())
    }

    #[test]
    fn host_rejects_credentials_routes_and_non_http_urls() {
        for input in [
            "",
            "javascript:alert(1)",
            "file:///tmp/ui",
            "https://user:pass@example.com",
            "https://example.com?token=secret",
            "https://example.com/#secret",
            "https://example.com/path",
            "https://example.com\n",
        ] {
            assert!(HostUrl::parse(input).is_err(), "accepted invalid host");
        }
    }

    #[test]
    fn host_accepts_explicit_local_development_origin() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            HostUrl::parse("http://127.0.0.1:3000/")?.origin(),
            "http://127.0.0.1:3000"
        );
        Ok(())
    }

    #[test]
    fn acknowledgement_requires_current_request_and_exact_contract()
    -> Result<(), Box<dyn std::error::Error>> {
        let value = serde_json::json!({"protocol":"shprd.shell.v1", "type":"ack", "request_id":"request-1"});
        let ack: BridgeAck = serde_json::from_value(value.clone())?;
        assert!(ack.accepts("request-1"));
        assert!(!ack.accepts("request-2"));
        let mut wrong = value;
        wrong["protocol"] = "other".into();
        assert!(serde_json::from_value::<BridgeAck>(wrong).is_err());
        Ok(())
    }
}
