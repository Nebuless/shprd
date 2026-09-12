# Settings service crate

## Purpose
Persistent GUI settings compatible with Herdr existing settings.json contract.

## Ownership
- This crate owns schema normalization, private atomic persistence, mutation serialization,
  connection/host settings-key ownership, repository hook preference, and auto-sync entries.
- Host owns default path selection, authentication, workspace resolution, paseo hook discovery,
  and auto-sync execution/notification.

## Local Contracts
- Construct with an explicit settings path. Never select or mutate the user home internally.
- Legacy keys remain local:<repo> and ssh:<host>:<repo> when connection ID is absent or
  legacy-default; non-legacy connections use URL-encoded connection:<id>: prefixes.
- Reject settings-file symlinks. Read malformed JSON as defaults; preserve malformed bytes until
  a caller explicitly persists an update.
- All read-modify-write mutations share one queue. Write private temporary files, sync them,
  then rename into place. Remove failed temporary files.
- Once persistence is queued, it owns the mutation lock until rename or cleanup even
  if the calling future is dropped. Cancellation before persistence makes no write.
- Reject keys outside current connection and host namespace. Host performs hook discovery and
  invokes auto-sync notification after successful updates.

## Verification
- cargo test --manifest-path crates/shprd-settings/Cargo.toml
- cargo fmt --manifest-path crates/shprd-settings/Cargo.toml -- --check
- cargo clippy --manifest-path crates/shprd-settings/Cargo.toml --all-targets -- -D warnings
- cargo run --manifest-path crates/shprd-settings/Cargo.toml --example settings -- SETTINGS_PATH

## Child DOX Index
- No child contracts.
