# SHPRD shell

## Purpose
Rust Dioxus connection shell for web and mobile; retained React runs in an iframe.

## Ownership
This crate owns shell UI, host validation and shell bridge. Host owns engines, Herdr workspaces and terminals. React owns editor, terminal and composer DOM.

## Local Contracts
- Never let Dioxus reconcile iframe document or React root.
- Host is an HTTP(S) origin without credentials, query, fragment or application path.
- Web builds default to `http://127.0.0.1:8787`, where `shprd-host` serves retained React and owns the local Herdr sockets. Native desktop and Android shells require an explicit reachable host; their loopback is the device.
- Bridge protocol `shprd.shell.v1` requires exact origin, owning window and current request ID. Never use wildcard targets or accept opaque origins.
- Host changes require confirmation; shell-only state updates must preserve draft.
- Bridge checks stay disabled until the current host iframe finishes loading.
- React integration installs `assets/react-bridge.mjs` with explicit allowed parent origin; cleanup removes its listener.

## Work Guidance
Follow DESIGN.md. Keep package metadata explicit for standalone `--manifest-path` checks. Coordinator owns root workspace integration.
Inspect inherited Android variables and the user-local SDK before declaring tools missing. Source android-env.sh for this workstation; do not substitute the legacy ~/Android/Sdk path.

## Verification
Run cargo test/check, cargo fmt --check, Dioxus web build, and `bun crates/shprd-shell/tests/browser.mjs` against built assets. The browser test needs `SHPRD_SHELL_DIST`, `SHPRD_REACT_MODULE_ROOT`, `SHPRD_PLAYWRIGHT_MODULE`, and `SHPRD_CHROME` on this workstation. Android requires real SDK, NDK and Rust target; browser width is not Android proof.

## Child DOX Index
None.
