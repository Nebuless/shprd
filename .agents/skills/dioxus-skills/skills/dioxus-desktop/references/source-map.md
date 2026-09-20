# Source map and gaps

Repository: `https://github.com/DioxusLabs/dioxus.git`

Exact SHA: `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`

Official explanation: <https://dioxuslabs.com/learn/0.7/guides/platforms/desktop>. Version label is Dioxus 0.7.0, while pinned local HEAD may contain later unreleased 0.7-line changes. Use guide for system-WebView model, native Rust distinction, eval concept, and asset overview. Use source below for names and behavior.

## Runtime

- `packages/desktop/src/launch.rs:51-224`: raw and normal launch, main-thread contract, runtime selection, event dispatch, default window wrapping.
- `packages/desktop/src/app.rs:24-260`: event-loop state, receiver setup, close and destroy policy, window map.
- `packages/desktop/src/config.rs:64-397`: `WindowConfig`, `Config`, window, protocols, navigation, and event-loop builders.
- `packages/desktop/src/webview.rs:253-559`: Tao window, Wry context and builder, IPC, navigation, drag-drop, menus, platform build.

## Windows and native

- `packages/desktop/src/desktop_context.rs:31-307`: context access, `DesktopService`, new window, close, native controls, handlers, shortcuts.
- `packages/desktop/src/desktop_state.rs:18-178`: app/window context, render-target queue, shutdown, Tao and Wry exposure.
- `packages/desktop/src/window_component.rs:14-164`: `WindowProps`, component-owned lifecycle, portal and providers.
- `packages/desktop/src/event_handlers.rs:6-69`: window event filtering and removal.
- `packages/desktop/src/hooks.rs:13-135`: window/app, Wry, menu, tray, asset, shortcut hooks.
- `packages/desktop/src/menubar.rs:4-127`: platform menu initialization and defaults.
- `packages/desktop/src/trayicon.rs:5-65`: tray types, creation, default menu, context hook.
- `examples/08-apis/{multiwindow.rs,custom_menu.rs,multiwindow_with_tray_icon.rs,window_event.rs}`: verified public usage.

## IPC and assets

- `packages/desktop/src/document.rs:26-129`: desktop `Document`, eval and message errors.
- `packages/desktop/src/ipc.rs:4-83`: event and internal message schema.
- `packages/desktop/src/protocol.rs:30-204`: protocol routing, index injection, loader, file dialogs.
- `packages/desktop/src/assets.rs:6-55`: named handler registry.
- `examples/08-apis/eval.rs:19-40`: public two-way eval use.

## Packaging and tests

- `packages/cli/src/cli/bundle.rs:11-139`: bundle command flow.
- `packages/cli/src/config/bundle.rs:130-433`: macOS, Windows, package type and signing/runtime settings.
- `packages/cli/src/bundler/{macos.rs,windows.rs,linux.rs}`: artifact creation.
- `packages/desktop/Cargo.toml:97-154`: renderer features and harness-free desktop tests.
- `packages/desktop/headless_tests/{utils.rs,multiwindow.rs,eval.rs,events.rs,forms.rs,rendering.rs}`: real hidden-WebView coverage.

## Known gaps

- Official guide doesn't cover current `Window` component ownership, headless root, current packaging matrix, menus, tray, or detailed failure behavior.
- Guide names `use_eval`; pinned API uses `document::eval`. Its Wry integration link mentions `use_window`, which still exists, but `window()` and direct context deref are also current.
- No unified desktop permission API exists in `dioxus-desktop`; permission declarations belong to OS package metadata and chosen native crate.
- Upstream hidden-window tests use fixed sleeps for DOM readiness and skip some Windows paths. Add event-based readiness and platform package smoke tests in applications.
- Linux dependency installation details vary by distro and Wry release. Confirm against pinned Wry and target distribution rather than inventing package names.
- Code signing, notarization, certificates, and store submission require vendor credentials and external platform tooling. Source exposes configuration, not credentials or approval workflow.
