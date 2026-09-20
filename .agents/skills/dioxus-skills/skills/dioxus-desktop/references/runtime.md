# Runtime and webview

Source pin: `DioxusLabs/dioxus@fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Choose entrypoint

- Normal app: `dioxus::launch(app)` or `dioxus::LaunchBuilder::desktop().with_cfg(config).launch(app)`. `launch` moves app-level settings into `App` and wraps root in default `Window`, unless `Config::with_headless_root(true)` is set. Source: `packages/desktop/src/launch.rs:199-224`, `packages/desktop/src/config.rs:321-370`.
- Raw VDOM: `launch_virtual_dom` and `launch_virtual_dom_blocking` create no implicit window. Root must render `Window` for UI. Blocking launch must run on main thread. Source: `packages/desktop/src/launch.rs:51-60`, `149-196`.
- Runtime: existing Tokio multi-thread runtime is accepted. Current-thread runtime panics because desktop event handling needs compatible scheduling. Without runtime, Dioxus creates multi-thread Tokio runtime when feature is enabled. Source: `packages/desktop/src/launch.rs:170-196`.

## Event loop

`App::new` consumes supplied `EventLoop<UserWindowEvent>` or builds one, creates proxy and DOM waker, starts with `ControlFlow::Wait`, installs HTML conversion plus desktop receivers, and holds one shared `VirtualDom`. Source: `packages/desktop/src/app.rs:24-103`.

Each Tao callback runs `App::tick`, then optional `Config::with_custom_event_handler`, then Dioxus dispatch. Built-in dispatch handles initialization, destruction, resize, VDOM polling, window creation and close, shutdown, menus, tray, hotkeys, hot reload, drag-drop glue, and IPC. Custom handler observes event but doesn't replace built-in dispatch. Source: `packages/desktop/src/launch.rs:60-146`, `packages/desktop/src/config.rs:389-397`.

Use `Config::with_event_loop` only when host must create Tao loop. Keep its `UserWindowEvent` type exact. Use `with_custom_event_handler` for host integration, not for intercepting or canceling Dioxus dispatch.

## Window and WebView build

`WindowConfig` owns `tao::window::WindowBuilder`, menu, protocols, index/head, data and resource directories, navigation, drag-drop, background, and pre-WebView callback. `Config` owns app-wide exit, event-loop, Wayland, tray-click, and root policy plus default `WindowConfig`. Source: `packages/desktop/src/config.rs:64-125`, `321-397`.

Build order matters:

1. Default desktop size becomes 800 by 600 when unset. Default icon is added. Tao window builds first.
2. `with_on_window` runs after native window creation and before WebView creation.
3. `WebContext` uses explicit data directory. Windows otherwise tries `%LOCALAPPDATA%/<exe_name>` to avoid unwritable executable directories.
4. Wry loads `dioxus://index.html/`, installs IPC and navigation handlers, then custom protocols, context-menu or devtools policy, menu, browser args, and platform-specific build method.
5. `DesktopService` receives WebView and window; initial redraw is requested.

Source: `packages/desktop/src/webview.rs:253-341`, `344-475`, `477-559`.

## Failure map

- No window: raw VDOM or headless root lacks mounted `Window`.
- Immediate panic: launch ran off main thread, Tao window build failed, Wry build failed, custom index lacks `</head>` or `</body>`, or Tokio runtime is current-thread. Sources: `launch.rs:58-60`, `170-189`; `webview.rs:279`, `539`; `protocol.rs:116-131`.
- Windows startup or cache failure: set writable `WindowConfig::with_data_directory`; don't put mutable WebView2 data under installed executable.
- HTML drag-drop absent on Windows: Wry handler blocks native HTML drag events. `with_disable_drag_drop_handler(true)` restores HTML API but loses Dioxus native file path bridge. Source: `config.rs:155-159`, `webview.rs:356-389`.
- External link replaced app: default handler opens `http`, `https`, and `mailto` externally and returns false. Custom navigation handler governs other non-Dioxus URLs. Source: `webview.rs:406-429`.
