use shprd_host::auth::Auth;

#[test]
fn malformed_suffix_beyond_token_read_limit_is_rejected() -> Result<(), Box<dyn std::error::Error>>
{
    // Given a valid-looking prefix hiding malformed data beyond the old read limit.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("auth-token");
    let contents = format!("{}{}invalid", "a".repeat(64), " ".repeat(64));
    std::fs::write(&path, &contents)?;
    // When loading the token, then the full malformed file is rejected and preserved.
    assert!(shprd_host::auth::load_or_create_token(&path).is_err());
    assert_eq!(std::fs::read_to_string(path)?, contents);
    Ok(())
}

#[test]
fn signed_cookie_authenticates_but_tampered_cookie_does_not()
-> Result<(), Box<dyn std::error::Error>> {
    // Given an authenticated login.
    let auth = Auth::new(true, "private-password".to_owned())?;
    let cookie = auth.login("private-password")?.ok_or("login failed")?;
    // When checking the issued credential, then it authenticates.
    assert!(auth.authenticated(Some(&cookie)));
    assert!(cookie.starts_with("herdr_auth="));
    // When its signature changes, then authentication fails.
    let invalid = cookie.replace("herdr_auth=", "herdr_auth=tampered");
    assert!(!auth.authenticated(Some(&invalid)));
    assert!(auth.login("wrong-password")?.is_none());
    Ok(())
}

#[test]
fn isolated_cookie_authenticates_only_its_cookie() -> Result<(), Box<dyn std::error::Error>> {
    let legacy = Auth::new(true, "legacy-secret".to_owned())?;
    let isolated = Auth::new_with_cookie(true, "shprd-secret".to_owned(), "shprd_auth")?;
    let legacy_cookie = legacy
        .login("legacy-secret")?
        .ok_or("legacy login failed")?;
    let isolated_cookie = isolated
        .login("shprd-secret")?
        .ok_or("isolated login failed")?;
    assert!(legacy_cookie.starts_with("herdr_auth="));
    assert!(isolated_cookie.starts_with("shprd_auth="));
    assert!(legacy.authenticated(Some(&legacy_cookie)));
    assert!(isolated.authenticated(Some(&isolated_cookie)));
    assert!(!legacy.authenticated(Some(&isolated_cookie)));
    assert!(!isolated.authenticated(Some(&legacy_cookie)));
    assert!(legacy.authenticated(Some(&format!("{legacy_cookie}; {isolated_cookie}"))));
    assert!(isolated.authenticated(Some(&format!("{legacy_cookie}; {isolated_cookie}"))));
    Ok(())
}

#[test]
fn empty_secret_cannot_enable_authentication() {
    // Given an exposed listener without a signing secret.
    // When constructing authentication, then startup fails closed.
    assert!(Auth::new(true, String::new()).is_err());
}

#[test]
fn token_persists_and_rejects_symlink() -> Result<(), Box<dyn std::error::Error>> {
    // Given a private configuration directory.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("auth-token");
    // When startup loads its secret again, then it preserves the credential.
    let first = shprd_host::auth::load_or_create_token(&path)?;
    assert_eq!(first.len(), 64);
    assert_eq!(first, shprd_host::auth::load_or_create_token(&path)?);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        assert_eq!(
            std::fs::metadata(&path)?.permissions().mode() & 0o777,
            0o600
        );
        let link = directory.path().join("symlink");
        symlink(&path, &link)?;
        assert!(shprd_host::auth::load_or_create_token(&link).is_err());
    }
    Ok(())
}
