# Agent history

## Purpose
Read-only native projection of Herdr-reported agent sessions.

## Ownership
Crate owns discovery, parsing, history revisions and ATIF export. Host owns authenticated
pane lookup and supplies trusted session paths; browser paths are not accepted directly.

## Local Contracts
- Explicit Herdr session paths may be outside default discovery roots.
- History window counts 200 conversation entries; associated tool rows remain visible.
- Redacted tool text reports UTF-8 byte length. Full payload stays available on demand.
- SSH commands quote the complete remote script, preserve strict host checks, bound
  stdout to 2 MiB and stderr to 64 KiB while reading, and kill children on cancellation.
- Never write session transcripts or start agent engines.

## Verification
Run cargo fmt, cargo clippy --all-targets -- -D warnings, cargo test, and
cargo run --example import_export with this crate manifest. SSH fixtures must reproduce
OpenSSH joining remote argv through a shell, including paths with spaces and quotes.

## Child DOX Index
None.
