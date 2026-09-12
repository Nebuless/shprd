use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn html_navigation_redirects_to_same_origin_login() -> Result<(), Box<dyn std::error::Error>>
{
    // Given a protected host behind a proxy.
    let home = tempfile::tempdir()?;
    let host = shprd_host::host::configured_router(
        home.path().join("missing.sock"),
        home.path().to_path_buf(),
        shprd_host::auth::Auth::new(true, "secret".to_owned())?,
    );
    // When an unauthenticated browser opens the application.
    let response = host
        .oneshot(
            Request::builder()
                .uri("/")
                .header("accept", "text/html")
                .header("x-forwarded-host", "attacker.invalid")
                .body(Body::empty())?,
        )
        .await?;
    // Then the relative redirect preserves the browser's public origin.
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        response
            .headers()
            .get("location")
            .ok_or("missing redirect")?,
        "/login"
    );
    Ok(())
}

#[tokio::test]
async fn local_login_does_not_require_a_password() -> Result<(), Box<dyn std::error::Error>> {
    // Given an intentionally unauthenticated local host.
    let home = tempfile::tempdir()?;
    let host = shprd_host::host::configured_router(
        home.path().join("missing.sock"),
        home.path().to_path_buf(),
        shprd_host::auth::Auth::new(false, String::new())?,
    );
    // When a client posts login without a credential.
    let response = host
        .oneshot(
            Request::builder()
                .uri("/api/login")
                .method("POST")
                .body(Body::empty())?,
        )
        .await?;
    // Then it succeeds without minting an empty-secret credential.
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("set-cookie").is_none());
    Ok(())
}

#[tokio::test]
async fn health_stays_available_without_downstream() -> Result<(), Box<dyn std::error::Error>> {
    // Given a host without a running Herdr instance.
    let host = shprd_host::host::router();
    // When checking its process health.
    let response = host
        .oneshot(Request::builder().uri("/health").body(Body::empty())?)
        .await?;
    // Then host health is independent of downstream readiness.
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(to_bytes(response.into_body(), 128).await?.as_ref(), b"Ok");
    Ok(())
}

#[tokio::test]
async fn remote_api_requires_login_and_cookie() -> Result<(), Box<dyn std::error::Error>> {
    // Given a password-protected host with an unavailable downstream.
    let home = tempfile::tempdir()?;
    let host = shprd_host::host::configured_router(
        home.path().join("missing.sock"),
        home.path().to_path_buf(),
        shprd_host::auth::Auth::new(true, "secret".to_owned())?,
    );
    // When fetching a protected route, then it requires authentication.
    let response = host
        .clone()
        .oneshot(Request::builder().uri("/api/health").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let login = host
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/login")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from("{\"password\":\"secret\"}"))?,
        )
        .await?;
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = login
        .headers()
        .get("set-cookie")
        .ok_or("missing cookie")?
        .clone();
    let response = host
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .header("cookie", cookie)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await?)?;
    assert_eq!(body["ok"], true);
    Ok(())
}
