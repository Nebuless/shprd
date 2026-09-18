use crate::{ConnectionId, Lease, Result, routing::routing};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpEndpoint {
    HerdrInfo,
    UploadImage,
    FetchImage,
    AgentSessionDownload,
    AgentSessionAtif,
    FileDownload,
    FileUpload,
    FileDelete,
}
const ENDPOINTS: [(HttpEndpoint, &str, &str); 8] = [
    (HttpEndpoint::HerdrInfo, "/herdr-info", "GET"),
    (HttpEndpoint::UploadImage, "/upload-image", "POST"),
    (HttpEndpoint::FetchImage, "/image-fetch", "GET"),
    (
        HttpEndpoint::AgentSessionDownload,
        "/agent-session/download",
        "GET",
    ),
    (HttpEndpoint::AgentSessionAtif, "/agent-session/atif", "GET"),
    (HttpEndpoint::FileDownload, "/file/download", "GET"),
    (HttpEndpoint::FileUpload, "/file/upload", "POST"),
    (HttpEndpoint::FileDelete, "/file/delete", "POST"),
];
#[derive(Debug)]
pub struct HttpRoute {
    pub endpoint: HttpEndpoint,
    pub connection_id: Option<ConnectionId>,
}
pub fn raw_pathname(url: &str) -> &str {
    let path = match url.find("://") {
        Some(scheme) => {
            let authority = &url[scheme + 3..];
            match authority.find('/') {
                Some(i) => &authority[i..],
                None => "/",
            }
        }
        None => {
            if url.starts_with('/') {
                url
            } else {
                "/"
            }
        }
    };
    path.split(['?', '#']).next().unwrap_or("/")
}
pub fn parse_http_route(path: &str, method: &str) -> Result<Option<HttpRoute>> {
    let (id, suffix) = if let Some(rest) = path.strip_prefix("/api/connections/") {
        let (encoded, suffix) = rest
            .split_once('/')
            .filter(|(id, _)| !id.is_empty())
            .ok_or_else(|| routing(400, "invalid connection route"))?;
        let mut bytes = Vec::new();
        let mut iter = encoded.bytes();
        while let Some(b) = iter.next() {
            if b == b'%' {
                let high = iter.next().and_then(|b| char::from(b).to_digit(16));
                let low = iter.next().and_then(|b| char::from(b).to_digit(16));
                let (Some(high), Some(low)) = (high, low) else {
                    return Err(routing(400, "invalid connection_id encoding"));
                };
                bytes.push(
                    u8::try_from(high * 16 + low)
                        .map_err(|_| routing(400, "invalid connection_id encoding"))?,
                );
            } else {
                bytes.push(b);
            }
        }
        let decoded =
            String::from_utf8(bytes).map_err(|_| routing(400, "invalid connection_id encoding"))?;
        (Some(ConnectionId::parse(&decoded)?), format!("/{suffix}"))
    } else if let Some(suffix) = path.strip_prefix("/api") {
        (None, suffix.to_owned())
    } else {
        return Ok(None);
    };
    let Some((endpoint, _, expected)) = ENDPOINTS.iter().find(|(_, s, _)| *s == suffix) else {
        return if id.is_some() {
            Err(routing(404, "unknown connection endpoint"))
        } else {
            Ok(None)
        };
    };
    if *endpoint == HttpEndpoint::FetchImage && id.is_none() {
        return Ok(None);
    }
    if method != *expected {
        return Err(routing(405, "method not allowed for connection endpoint"));
    }
    Ok(Some(HttpRoute {
        endpoint: *endpoint,
        connection_id: id,
    }))
}
pub fn response_headers(lease: &Lease) -> [(String, String); 2] {
    [
        (
            "X-Herdr-Connection-Id".into(),
            lease.connection_id.as_str().into(),
        ),
        (
            "X-Herdr-Connection-Generation".into(),
            lease.generation().to_string(),
        ),
    ]
}
