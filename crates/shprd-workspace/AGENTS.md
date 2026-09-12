# Workspace service crate

## Purpose
Rust file and Git services for checkouts resolved by the connection host.

## Ownership
- This crate owns file operations, Git commands, previews, and activity snapshots.
- Host owns Herdr workspace/pane resolution, auth, HTTP routing, and connection leases.

## Local Contracts
- One WorkspaceService per connection generation; never share across SSH destinations.
- Explorer paths stay inside canonical checkout. Explicit absolute read/resolve paths
  preserve the existing preview contract; downloads, deletes, and uploads remain confined.
- Git paths are literal pathspecs. Destructive actions require live status checks and
  summary fingerprints or the existing missing-file exception.
- Invoke capture_workspace before agent edits and complete_workspace at cycle end.
- Keep Download alive while streaming its body; it owns temporary archive cleanup.

## Verification
- cargo test --manifest-path crates/shprd-workspace/Cargo.toml
- cargo fmt --manifest-path crates/shprd-workspace/Cargo.toml -- --check
- cargo clippy --manifest-path crates/shprd-workspace/Cargo.toml --all-targets -- -D warnings
- cargo run --manifest-path crates/shprd-workspace/Cargo.toml --example rpc -- CHECKOUT file.list '{}'

## Child DOX Index
- No child contracts.
