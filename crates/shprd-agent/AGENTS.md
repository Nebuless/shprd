# Native attachment client

## Purpose
Discover and control existing Senpi and Atomic runtime attachments through authenticated local sockets.

## Ownership
- Own catalog, typed commands, bounded transport and event envelopes.
- Native extension calls live in integrations/pi; host routing is coordinator-owned.

## Local Contracts
- Never launch engines or write session JSONL.
- Never expose discovery tokens to browser clients or logs.
- Authenticate get_state before exposing discovered sessions.
- Never replay mutations after connection loss; delivery may already have occurred.
- Drop Attachment after failed or cancelled request/event reads; reconnect and refresh snapshots.

## Verification
- cargo test --manifest-path crates/shprd-agent/Cargo.toml
- cargo check --manifest-path crates/shprd-agent/Cargo.toml
- cargo build --manifest-path crates/shprd-agent/Cargo.toml --example probe

## Child DOX Index
None.
