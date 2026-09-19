# SHPRD shell

## Purpose
Rust Dioxus shell for web and mobile. Web hosts retained React in an iframe; Android opens the packaged retained React entry locally.

## Ownership
This crate owns shell UI, host validation and shell bridge. Host owns engines, Herdr workspaces and terminals. React owns editor, terminal and composer DOM.

## Local Contracts
- Never let Dioxus reconcile the iframe document or React root.
- Host is an HTTP(S) origin without credentials, query, fragment or application path.
- Web builds default to `http://127.0.0.1:8787`, where `shprd-host` serves retained React and owns the local Herdr sockets. Native desktop and Android shells require an explicit reachable host; their loopback is the device.
- Bridge protocol `shprd.shell.v1` requires exact origin, owning window and current request ID. Never use wildcard targets or accept opaque origins.
- Android packages `server/public` and mounts React directly at `#root`; Dioxus uses a separate missing root name so it never reconciles React DOM.
- Host changes from React require confirmation; bridge checks preserve the current iframe document and draft.
- `assets/shell-controls.js` accepts only exact packets from current iframe source and origin. React installs `web/src/shellBridge.ts` from the origin-only referrer, then cleans its listener on unmount.

## Work Guidance
Follow DESIGN.md. Keep package metadata explicit for standalone `--manifest-path` checks. Coordinator owns root workspace integration.
Inspect inherited Android variables and the user-local SDK before declaring tools missing. Source android-env.sh for this workstation; do not substitute the legacy ~/Android/Sdk path.
Use `scripts/build-android.sh` for release APKs. It builds existing Vite output, links that output into the generated Android asset source, and runs Gradle packaging without creating another frontend tree.

## Verification
Run cargo test/check, cargo fmt --check, Dioxus web build, and `bun crates/shprd-shell/tests/browser.mjs` against built assets. The browser test needs `SHPRD_SHELL_DIST`, `SHPRD_REACT_MODULE_ROOT`, `SHPRD_PLAYWRIGHT_MODULE`, and `SHPRD_CHROME` on this workstation. Android requires real SDK, NDK and Rust target; run `bun test scripts/android-package.test.ts` against the release APK. Browser width is not Android proof.

## Child DOX Index
None.
