---
name: dioxus-desktop
description: Use when building or diagnosing Dioxus desktop launch, event loops, Wry or Tao integration, windows, menus, tray icons, events, Rust-JavaScript IPC, assets, native permissions, packaging, or tests.
metadata:
  invocation: model
---

# Dioxus desktop

1. Verify `/root/repo/dioxus` is at `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`. Treat that source as authority. The official [0.7 desktop guide](https://dioxuslabs.com/learn/0.7/guides/platforms/desktop) is explanatory and may describe an earlier 0.7 API. In particular, use pinned `document::eval`, `use_window`, and `window()` symbols rather than copying its `use_eval` wording.
2. Choose ownership before writing code. Use normal `dioxus::launch` or `LaunchBuilder::desktop()` for one implicit window. Use `Config::with_headless_root(true)` plus keyed `Window` components when root state owns every window. Use `DesktopAppContext::new_window` or `DesktopService::new_window` only when imperative creation is required.
3. Load only matching reference:
   - [`references/runtime.md`](references/runtime.md) for launch, Tao event loop, Wry webview construction, `Config`, `WindowConfig`, and failure diagnosis.
   - [`references/windows-native.md`](references/windows-native.md) for `DesktopContext`, component windows, close lifecycle, events, menus, tray, shortcuts, permissions, and native handles.
   - [`references/ipc-assets.md`](references/ipc-assets.md) for `document::eval`, Rust-JavaScript messages, internal IPC, navigation, custom protocols, and assets.
   - [`references/packaging-testing.md`](references/packaging-testing.md) for bundling, signing, WebView prerequisites, headless tests, and platform edge cases.
   - [`references/source-map.md`](references/source-map.md) when checking a claim, resolving drift, or finding known gaps.
4. Preserve event-loop rules. Launch on main thread, keep native window work on event-loop path, and don't use Tokio current-thread runtime. Match handlers by `WindowId`; remove scoped handlers through hook cleanup.
5. Treat webview input as untrusted. Parse messages into typed Rust values, allow navigation deliberately, keep native capabilities in Rust, and expose only narrow custom protocols or eval channels needed by feature.
6. Test through real desktop surface. Exercise launch, one user action, close or shutdown, and one failure path on each supported OS. Use invisible windows only for automation, not as substitute for packaged-app smoke test.

Finish when workflow compiles against pinned source, observed window behavior matches close and event ownership, IPC errors are handled, package artifact launches on target OS, and every API claim traces to [`references/source-map.md`](references/source-map.md).
