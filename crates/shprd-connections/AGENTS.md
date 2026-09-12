# Native connections

## Purpose
Rust profile persistence, runtime generation leases, routing contracts, and SSH process ownership.

## Ownership
Only this crate owns these native helpers. Coordinator owns root Cargo integration and host dispatch.

## Local Contracts
- Validate JSON profiles and registry versions before persistence or runtime construction.
- Default paths may harden permissions; explicit paths require private permissions.
- Never read or mutate live profiles in tests; use unique temporary directories and isolated executables.
- Runtime start must be cancellation-safe; stop must clean partial startup and reap subprocesses.
- Invalidate generation leases before cleanup. Check leases after awaited reads and before publishing each chunk.
- Runtime factories and control/render protocol probes are host callbacks. Protocols 14-20 and 22 only.
- Host supervision consumes RetryPolicy tickets, checks currency after cleanup, and cancels tickets on explicit disconnect/update/remove/shutdown.

## Verification
- cargo test --manifest-path crates/shprd-connections/Cargo.toml
- cargo fmt --manifest-path crates/shprd-connections/Cargo.toml -- --check
- cargo clippy --manifest-path crates/shprd-connections/Cargo.toml --all-targets -- -D warnings

## Child DOX Index
None.
