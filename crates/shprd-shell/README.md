# SHPRD shell

Dioxus 0.7.10 owns the shell. Web mode retains the configured host origin in an iframe. Android starts with a packaged bridge picker, then loads the selected bridge in its WebView. React alone owns its editor, terminal and composer; engines and Herdr stay on the host.

## Build

From this crate:

```sh
cargo test --lib
cargo check --target wasm32-unknown-unknown --features web
dx build --platform web --no-default-features --features web
```

Optional `SHPRD_HOST_URL=https://host.example` at compile time supplies the initial host. Otherwise enter the host through the Rust form. Only an HTTP(S) origin is accepted; credentials, query strings, fragments and application paths are rejected. Host settings are in memory, not persisted. Changing host requires confirmation because navigation discards unsaved React state. Editing settings or checking the bridge leaves the iframe mounted.

The signed Android APK is built from this shell. Its picker navigates to the selected network-reachable SHPRD bridge so login cookies, API calls, and WebSockets remain first-party to that bridge. It cannot start a Herdr bridge or reach a loopback-only service on the phone.

For a local release build, source the Android setup from this crate, then run:

```sh
. ./crates/shprd-shell/android-env.sh
rustup target add aarch64-linux-android x86_64-linux-android
mise run shell:android
```

`android-env.sh` exports the installed SDK at `~/.local/share/android-sdk`, NDK 29.0.14206865 and mise Temurin JDK 21. Existing ANDROID_HOME, ANDROID_NDK_HOME and JAVA_HOME overrides take precedence. Other machines should override those paths for their installed toolchain. Release automation supplies its signing key through GitHub Actions secrets; local builds must sign the generated release APK before distribution.

Use HTTPS for a remote host. Native platform frame origin, cookie policy, keyboard, IME and device behavior require Android verification. This crate does not start local agent engines or implement a custom webview.

## Coordinator integration

Add `crates/shprd-shell` to root Cargo workspace members. Explicit package metadata needs no workspace inheritance. Root `Dioxus.toml` supplies SHPRD application metadata when run from the integrated workspace:

```sh
dx build --package shprd-shell --platform web --no-default-features --features web
```

Serve retained React at the configured host root. Install the bridge module in its entry point with an explicit trusted shell origin:

```js
import { installShellBridge } from "../../crates/shprd-shell/assets/react-bridge.mjs";
const disposeShellBridge = installShellBridge(configuredShellOrigin);
// Call disposeShellBridge when that entry point is torn down or hot-replaced.
```

The host must allow framing by that shell origin in response headers. Do not derive trust from arbitrary query parameters, use wildcard postMessage targets, or accept opaque origins. Authentication remains inside the host document; no credentials enter shell URLs or bridge messages. Cross-site cookies may need host-specific deployment settings.

Protocol `shprd.shell.v1`: shell sends `{protocol,type:"ping",request_id}`; React replies `{protocol,type:"ack",request_id}`. Both endpoints check exact message shape, origin and owning window. Shell also requires the current request ID, cleans up on acknowledgement/navigation/timeout, and reports missing integration instead of claiming a host connection.

## Browser verification

Set `SHPRD_REACT_MODULE_ROOT` to existing web/node_modules, `SHPRD_PLAYWRIGHT_MODULE` to installed playwright-core/index.mjs, `SHPRD_CHROME` to the Chrome executable, and `SHPRD_REACT_DIST` to retained product assets. Run `bun crates/shprd-shell/tests/browser.mjs` from repository root. Optional `SHPRD_SHELL_DIST` overrides Dioxus debug web output.

Tests exercise controlled React draft state, exact bridge provenance, listener cleanup, cancelled navigation, invalid host input and desktop/mobile-width screenshots. `SHPRD_REACT_DIST` renders the existing product bundle and installs the bridge in the test server response without modifying that bundle. Static rendering does not prove host RPC, terminal or Android device behavior.
