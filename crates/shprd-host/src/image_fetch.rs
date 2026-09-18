//! SSRF-resistant image fetcher: HTTPS, pinned DNS connection, bounded raster data.

use http_body_util::{BodyExt, Empty};
use hyper::{
    Request, StatusCode,
    body::{Bytes, Incoming},
    client::conn::http1,
};
use hyper_util::rt::TokioIo;
use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::{
    net::{TcpStream, lookup_host},
    time::{Duration, timeout},
};
use tokio_rustls::TlsConnector;
use url::Url;

pub const MAX_BYTES: usize = 25 * 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const FETCH_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FetchError {
    #[error("invalid image URL")]
    InvalidUrl,
    #[error("image URL blocked")]
    BlockedUrl,
    #[error("image DNS resolution blocked")]
    BlockedAddress,
    #[error("image network request failed")]
    Network,
    #[error("image response invalid")]
    InvalidResponse,
    #[error("image too large")]
    TooLarge,
    #[error("unsupported image")]
    UnsupportedImage,
    #[error("connection changed during image fetch")]
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageData {
    pub mime_type: &'static str,
    pub data: Vec<u8>,
    pub cache_control: &'static str,
}

pub async fn fetch_image(url: &str, current: impl Fn() -> bool) -> Result<ImageData, FetchError> {
    within_total_timeout(
        FETCH_TOTAL_TIMEOUT,
        fetch_image_inner(url, &current),
    )
    .await
}

async fn fetch_image_inner(
    url: &str,
    current: &impl Fn() -> bool,
) -> Result<ImageData, FetchError> {
    let mut url = parse_url(url)?;
    for redirect in 0..=3 {
        ensure_current(current)?;
        let (status, headers, body) = fetch_once(&url, current).await?;
        if let Some(next) = redirect_destination(
            &url,
            status,
            headers.get("location").and_then(|value| value.to_str().ok()),
            redirect,
        )? {
            url = next;
            continue;
        }
        if status != StatusCode::OK {
            return Err(FetchError::InvalidResponse);
        }
        let mime = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .ok_or(FetchError::InvalidResponse)?;
        let data = collect_body(body, headers.get("content-length"), current).await?;
        ensure_current(current)?;
        return identify(data, mime);
    }
    Err(FetchError::InvalidResponse)
}

fn parse_url(raw: &str) -> Result<Url, FetchError> {
    let url = Url::parse(raw).map_err(|_| FetchError::InvalidUrl)?;
    validate_url(&url)?;
    Ok(url)
}

fn redirect_destination(
    current: &Url,
    status: StatusCode,
    location: Option<&str>,
    redirect: u8,
) -> Result<Option<Url>, FetchError> {
    if !status.is_redirection() {
        return Ok(None);
    }
    if redirect >= 3 {
        return Err(FetchError::InvalidResponse);
    }
    let target = current
        .join(location.ok_or(FetchError::InvalidResponse)?)
        .map_err(|_| FetchError::InvalidUrl)?;
    validate_url(&target)?;
    Ok(Some(target))
}

fn validate_url(url: &Url) -> Result<(), FetchError> {
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || url.username() != ""
        || url.password().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        return Err(FetchError::InvalidUrl);
    }
    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".ts.net")
    {
        return Err(FetchError::BlockedUrl);
    }
    Ok(())
}

async fn fetch_once(
    url: &Url,
    current: &impl Fn() -> bool,
) -> Result<(StatusCode, hyper::HeaderMap, Incoming), FetchError> {
    let host = url.host_str().ok_or(FetchError::InvalidUrl)?;
    let addrs: Vec<SocketAddr> = while_current(current, lookup_host((host, 443)))
        .await?
        .map_err(|_| FetchError::Network)?
        .collect();
    let addr = pinned_address(&addrs)?;
    let stream = while_current(current, TcpStream::connect(addr))
        .await?
        .map_err(|_| FetchError::Network)?;
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = ServerName::try_from(host.to_owned()).map_err(|_| FetchError::InvalidUrl)?;
    let tls = while_current(
        current,
        TlsConnector::from(Arc::new(config)).connect(name, stream),
    )
    .await?
    .map_err(|_| FetchError::Network)?;
    let (mut sender, connection) = while_current(current, http1::handshake(TokioIo::new(tls)))
        .await?
        .map_err(|_| FetchError::Network)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let path = match url[url::Position::BeforePath..].find('?') {
        Some(_) => format!("{}?{}", url.path(), url.query().unwrap_or_default()),
        None => url.path().to_owned(),
    };
    let request = Request::builder()
        .uri(path)
        .header("host", host)
        .body(Empty::<Bytes>::new())
        .map_err(|_| FetchError::Network)?;
    let response = while_current(current, sender.send_request(request))
        .await?
        .map_err(|_| FetchError::Network)?;
    Ok((
        response.status(),
        response.headers().clone(),
        response.into_body(),
    ))
}

async fn collect_body(
    body: Incoming,
    length: Option<&hyper::header::HeaderValue>,
    current: &impl Fn() -> bool,
) -> Result<Vec<u8>, FetchError> {
    collect_body_frames(body, length, current).await
}

async fn collect_body_frames<B>(
    mut body: B,
    length: Option<&hyper::header::HeaderValue>,
    current: &impl Fn() -> bool,
) -> Result<Vec<u8>, FetchError>
where
    B: hyper::body::Body<Data = Bytes> + Unpin,
{
    ensure_current(current)?;
    if let Some(length) = length {
        let length = length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(FetchError::InvalidResponse)?;
        if length > MAX_BYTES {
            return Err(FetchError::TooLarge);
        }
    }
    let mut out = Vec::new();
    while let Some(frame) = while_current(current, body.frame()).await? {
        let frame = frame.map_err(|_| FetchError::Network)?;
        if let Some(chunk) = frame.data_ref() {
            if out
                .len()
                .checked_add(chunk.len())
                .is_none_or(|n| n > MAX_BYTES)
            {
                return Err(FetchError::TooLarge);
            }
            out.extend_from_slice(chunk);
        }
    }
    Ok(out)
}

fn ensure_current(current: &impl Fn() -> bool) -> Result<(), FetchError> {
    if current() {
        Ok(())
    } else {
        Err(FetchError::Stale)
    }
}

async fn while_current<T>(
    current: &impl Fn() -> bool,
    future: impl Future<Output = T>,
) -> Result<T, FetchError> {
    ensure_current(current)?;
    let result = within(future).await?;
    ensure_current(current)?;
    Ok(result)
}

async fn within<T>(future: impl Future<Output = T>) -> Result<T, FetchError> {
    within_timeout(FETCH_TIMEOUT, future).await
}

async fn within_timeout<T>(
    duration: Duration,
    future: impl Future<Output = T>,
) -> Result<T, FetchError> {
    timeout(duration, future).await.map_err(|_| FetchError::Network)
}

async fn within_total_timeout<T>(
    duration: Duration,
    future: impl Future<Output = Result<T, FetchError>>,
) -> Result<T, FetchError> {
    timeout(duration, future)
        .await
        .map_err(|_| FetchError::Network)?
}

fn identify(data: Vec<u8>, content_type: &str) -> Result<ImageData, FetchError> {
    let (mime, ok) = if data.len() >= 33
        && data.starts_with(b"\x89PNG\r\n\x1a\n")
        && data[8..12] == [0, 0, 0, 13]
        && &data[12..16] == b"IHDR"
        && data[16..24] != [0; 8]
    {
        ("image/png", content_type_matches(content_type, "image/png"))
    } else if data.len() >= 4
        && data.starts_with(&[0xff, 0xd8, 0xff])
        && data.ends_with(&[0xff, 0xd9])
    {
        (
            "image/jpeg",
            content_type_matches(content_type, "image/jpeg"),
        )
    } else if data.len() >= 13
        && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"))
        && data[6..10] != [0; 4]
    {
        ("image/gif", content_type_matches(content_type, "image/gif"))
    } else if data.len() >= 20
        && &data[..4] == b"RIFF"
        && &data[8..12] == b"WEBP"
        && matches!(&data[12..16], b"VP8 " | b"VP8L" | b"VP8X")
    {
        (
            "image/webp",
            content_type_matches(content_type, "image/webp"),
        )
    } else if data.len() >= 54 && data.starts_with(b"BM") {
        ("image/bmp", content_type_matches(content_type, "image/bmp"))
    } else if data.len() >= 22 && data[..4] == [0, 0, 1, 0] && data[4..6] != [0, 0] {
        (
            "image/x-icon",
            content_type_matches(content_type, "image/x-icon"),
        )
    } else if data.len() >= 16
        && &data[4..8] == b"ftyp"
        && (&data[8..12] == b"avif" || &data[8..12] == b"avis")
    {
        (
            "image/avif",
            content_type_matches(content_type, "image/avif"),
        )
    } else {
        return Err(FetchError::UnsupportedImage);
    };
    if !ok {
        return Err(FetchError::InvalidResponse);
    }
    shprd_agent::validate_image_bytes(mime, &data)
        .map_err(|_| FetchError::UnsupportedImage)?;
    Ok(ImageData {
        mime_type: mime,
        data,
        cache_control: "no-store",
    })
}

fn content_type_matches(value: &str, expected: &str) -> bool {
    value.split(';').next().is_some_and(|value| {
        let value = value.trim();
        value.eq_ignore_ascii_case(expected)
            || (expected == "image/x-icon"
                && value.eq_ignore_ascii_case("image/vnd.microsoft.icon"))
    })
}

fn pinned_address(addrs: &[SocketAddr]) -> Result<SocketAddr, FetchError> {
    if addrs.is_empty() || addrs.iter().any(|address| !is_public(address.ip())) {
        return Err(FetchError::BlockedAddress);
    }
    Ok(addrs[0])
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let [a, b, c, _] = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_multicast()
                || v.is_broadcast()
                || a == 0
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0)
                || (a == 192 && b == 0 && c == 2)
                || (a == 192 && b == 31 && c == 196)
                || (a == 192 && b == 52 && c == 193)
                || (a == 192 && b == 88 && c == 99)
                || (a == 192 && b == 175 && c == 48)
                || (a == 198 && (b == 18 || b == 19))
                || (a == 198 && b == 51 && c == 100)
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4() {
                return is_public(IpAddr::V4(v4));
            }
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (v.segments()[0] & 0xfe00) == 0xfc00
                || (v.segments()[0] & 0xffc0) == 0xfe80
                || (v.segments()[0] == 0x0064 && v.segments()[1] == 0xff9b)
                || (v.segments()[0] == 0x0100 && v.segments()[1] == 0)
                || (v.segments()[0] == 0x2001
                    && (v.segments()[1] < 0x0200
                        || (v.segments()[1] & 0xfff0) == 0x0020
                        || v.segments()[1] == 0x0db8))
                || v.segments()[0] == 0x2002
                || (v.segments()[0] & 0xfff0) == 0x3ff0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_policy() {
        for url in [
            "http://example.com/x",
            "https://user@example.com/x",
            "https://example.com:444/x",
            "https://example.com/x#fragment",
        ] {
            assert!(
                matches!(parse_url(url), Err(FetchError::InvalidUrl)),
                "{url}"
            );
        }
        for url in [
            "https://localhost/x",
            "https://foo.localhost/x",
            "https://foo.local/x",
            "https://foo.internal/x",
            "https://foo.ts.net/x",
        ] {
            assert!(
                matches!(parse_url(url), Err(FetchError::BlockedUrl)),
                "{url}"
            );
        }
        assert!(matches!(parse_url("https://example.com/x"), Ok(_)));
        let current = parse_url("https://example.com/one")
            .unwrap_or_else(|error| panic!("fixture URL: {error}"));
        assert!(matches!(
            redirect_destination(
                &current,
                StatusCode::FOUND,
                Some("https://localhost/two"),
                0,
            ),
            Err(FetchError::BlockedUrl)
        ));
        let mut url = current;
        for redirect in 0..3 {
            let Some(next) = redirect_destination(
                &url,
                StatusCode::FOUND,
                Some("/again"),
                redirect,
            )
            .unwrap_or_else(|error| panic!("redirect {redirect}: {error}"))
            else {
                panic!("redirect {redirect}: missing destination");
            };
            url = next;
        }
        assert!(matches!(
            redirect_destination(&url, StatusCode::FOUND, Some("/again"), 3),
            Err(FetchError::InvalidResponse)
        ));
        assert_eq!(
            redirect_destination(&url, StatusCode::OK, None, 0),
            Ok(None)
        );
    }
    fn address(value: &str) -> IpAddr {
        match value.parse() {
            Ok(address) => address,
            Err(error) => panic!("invalid IP test fixture {value}: {error}"),
        }
    }
    #[test]
    fn ip_policy() {
        for value in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "192.0.0.1",
            "192.0.2.1",
            "192.31.196.1",
            "192.52.193.1",
            "192.88.99.1",
            "192.175.48.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "::1",
            "64:ff9b::1",
            "100::1",
            "2001:20::1",
            "2001:db8::1",
            "2002:c000:0201::1",
            "3fff::1",
        ] {
            assert!(!is_public(address(value)), "{value}");
        }
        for value in ["8.8.8.8", "2001:4860:4860::8888"] {
            assert!(is_public(address(value)), "{value}");
        }
        let public: SocketAddr = "8.8.8.8:443"
            .parse()
            .unwrap_or_else(|error| panic!("fixture address: {error}"));
        let private: SocketAddr = "127.0.0.1:443"
            .parse()
            .unwrap_or_else(|error| panic!("fixture address: {error}"));
        let public_v6: SocketAddr = "[2001:4860:4860::8888]:443"
            .parse()
            .unwrap_or_else(|error| panic!("fixture address: {error}"));
        let private_v6: SocketAddr = "[::1]:443"
            .parse()
            .unwrap_or_else(|error| panic!("fixture address: {error}"));
        assert!(matches!(pinned_address(&[]), Err(FetchError::BlockedAddress)));
        assert_eq!(pinned_address(&[public]), Ok(public));
        assert!(matches!(
            pinned_address(&[public, private]),
            Err(FetchError::BlockedAddress)
        ));
        assert!(matches!(
            pinned_address(&[public_v6, private_v6]),
            Err(FetchError::BlockedAddress)
        ));
    }
    #[tokio::test]
    async fn total_fetch_deadline_stops_slow_retrieval() {
        assert_eq!(
            within_total_timeout(Duration::ZERO, std::future::pending::<Result<(), FetchError>>())
                .await,
            Err(FetchError::Network)
        );
    }

    #[tokio::test]
    async fn stale_lease_and_timeout_abort_without_waiting() {
        assert_eq!(ensure_current(&|| false), Err(FetchError::Stale));
        let active = std::cell::Cell::new(true);
        let current = || active.get();
        assert_eq!(
            while_current(&current, async {
                active.set(false);
                ()
            })
            .await,
            Err(FetchError::Stale)
        );
        let checks = std::cell::Cell::new(0);
        let current = || {
            checks.set(checks.get() + 1);
            checks.get() == 1
        };
        assert_eq!(
            collect_body_frames(
                http_body_util::Full::new(Bytes::from_static(b"image")),
                None,
                &current,
            )
            .await,
            Err(FetchError::Stale)
        );
        assert_eq!(
            within_timeout(Duration::ZERO, std::future::pending::<()>()).await,
            Err(FetchError::Network)
        );
    }
    #[test]
    fn signatures_and_types() {
        let png = include_bytes!("../../../site/assets/herdr-icon-48.png").to_vec();
        assert!(content_type_matches(" IMAGE/PNG ; charset=binary ", "image/png"));
        assert!(shprd_agent::validate_image_bytes("image/png", &png).is_ok());
        assert!(matches!(
            identify(png.clone(), " IMAGE/PNG ; charset=binary "),
            Ok(ImageData {
                mime_type: "image/png",
                ..
            })
        ));
        for (data, content_type) in [
            (b"\x89PNG\r\n\x1a\n".as_slice(), "image/png"),
            (&png[..png.len() - 1], "image/png"),
            (b"\xff\xd8\xff\xe0".as_slice(), "image/jpeg"),
            (b"GIF89a".as_slice(), "image/gif"),
            (b"BMdata".as_slice(), "image/png"),
            (&[0, 0, 1, 0][..], "image/x-icon"),
            (b"nope".as_slice(), "image/png"),
        ] {
            assert!(
                identify(data.to_vec(), content_type).is_err(),
                "accepted malformed {content_type}: {} bytes",
                data.len()
            );
        }
    }
}
