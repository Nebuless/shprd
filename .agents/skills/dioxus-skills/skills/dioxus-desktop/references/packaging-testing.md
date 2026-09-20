# Packaging and testing

Source pin: `DioxusLabs/dioxus@fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Package

Run `dx bundle --desktop` for host defaults. Add `--package-types` when exact artifact matters. Pinned CLI supports `macos`, `dmg`, `msi`, `nsis`, `deb`, `rpm`, `appimage`, and `updater` desktop package types. CLI builds first, fills platform defaults, validates requested package types, invokes package bundler, then prints artifact paths. Sources: `packages/cli/src/cli/bundle.rs:11-139`; `packages/cli/src/config/bundle.rs:363-433`.

Configure metadata and resources in `Dioxus.toml`; inspect pinned schema before naming keys. Platform bundle code owns layout and signing:

- macOS `.app`: `packages/cli/src/bundler/macos.rs`, with signing identity, entitlements, Info.plist, hardened runtime, frameworks, and files from `packages/cli/src/config/bundle.rs:152-217`.
- Windows: MSI and NSIS in `packages/cli/src/bundler/windows.rs`; WebView2 install defaults to offline installer and can be skip, bootstrapper, offline, or fixed runtime. Source: `packages/cli/src/config/bundle.rs:220-340`.
- Linux: AppImage, Debian, RPM in `packages/cli/src/bundler/linux.rs`. Build and smoke test on target distro because WebKitGTK and desktop integration vary.

Package success means artifact installs and launches on clean target. Also test asset loading, writable WebView data, menu/tray, protocol links, permission denial, second launch if app enforces singleton, and uninstall or upgrade identity. Windows MSI upgrade code must stay stable. Source: `packages/cli/src/config/bundle.rs:130-149`.

## Test

Desktop tests need main thread, so upstream headless tests set `harness = false`. Invisible Tao window still creates real Wry WebView. A deadman's switch exits stuck process. Sources: `packages/desktop/Cargo.toml:130-154`; `packages/desktop/headless_tests/utils.rs:6-26`.

Recommended layers:

1. Pure Rust unit tests for state and message parsing.
2. Renderer test with hidden window, real WebView, bounded shutdown, and observable DOM or native result.
3. Multiwindow test covering mount, close callback, portal removal, replacement window, and app shutdown. Upstream shape: `packages/desktop/headless_tests/multiwindow.rs:15-88`.
4. Packaged smoke test on each target OS. Verify installation and native integration, not only dev launch.

Avoid fixed sleeps as readiness signal in new tests. Wait for mounted DOM, IPC response, window event, or explicit channel with overall timeout. Upstream test has long sleeps and comment admitting mount synchronization gap at `packages/desktop/headless_tests/utils.rs:34-54`; treat it as known gap, not pattern.

## Edge checklist

- Launch and native UI on main thread.
- Last window closes versus hides, tray app remains reachable.
- Component close callback removes owner state; imperative close doesn't leave strong cycle.
- Custom index has required tags and matching root ID.
- Windows WebView2 data path writable; runtime installed by chosen package mode.
- Windows drag-drop policy chosen explicitly.
- Linux Wayland and X11 tested separately when using shortcuts or DMA-BUF.
- Release build tested because context menu and devtools differ from debug.
- Signed package tested for camera, location, notifications, filesystem, protocol handlers, and other OS permissions used by app.
- External navigation allowlist tested with blocked URL.
