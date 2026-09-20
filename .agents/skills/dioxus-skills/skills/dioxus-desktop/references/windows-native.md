# Windows and native integration

Source pin: `DioxusLabs/dioxus@fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Handles

- `window()` consumes current `DesktopContext` and panics outside Dioxus app context. `use_window()` retains same context as hook. `app()` and `use_app()` return app-wide `Rc<DesktopAppContext>`. Sources: `packages/desktop/src/desktop_context.rs:31-57`; `packages/desktop/src/hooks.rs:13-21`.
- `DesktopContext` is `Rc<DesktopService>`. It dereferences through `DesktopWindowContext` to Tao `Window`, so methods such as `set_minimized`, `set_resizable`, and `request_redraw` work directly. Public `webview` and `window` fields expose Wry and Tao. Sources: `desktop_context.rs:51-57`, `102-114`; `desktop_state.rs:82-98`, `170-178`.
- Avoid strong-reference cycles between windows. `WeakDesktopContext` exists because retained `Rc` handles can keep Tao window alive. Source: `desktop_context.rs:54-57`, `164-198`.

## Multiwindow lifecycle

Preferred declarative pattern: `Config::with_headless_root(true)`, keyed `Window` components from state, `WindowConfig` per window, and `onclose` removing key. Children share parent context and state but receive window-specific desktop document, memory history, and context through portal. Sources: `examples/08-apis/multiwindow.rs:9-33`; `packages/desktop/src/window_component.rs:49-74`, `74-164`.

Window creation is queued by `DesktopAppContext::new_window`: create render target, send `UserWindowEvent::NewWindow`, queue pending WebView. `DesktopService::new_window` returns `PendingDesktopWindow` because WebView2 forbids creating new window reentrantly inside existing callback. Await before controlling it. Sources: `desktop_state.rs:57-65`; `desktop_context.rs:164-198`.

Close paths:

- `WindowHides` sets visibility false. App stays alive.
- `WindowCloses` asks component owner first. Component `onclose` updates state, portal tears down, then native window is destroyed.
- Unowned/root window closes immediately. With `exit_on_last_window_close`, empty WebView map exits loop.
- `DesktopAppContext::shutdown` sends app-wide `Shutdown`.

Sources: `packages/desktop/src/app.rs:183-237`; `desktop_state.rs:28-40`, `68-71`; `window_component.rs:91-164`.

## Events, menus, tray, shortcuts

Use `use_wry_event_handler` for Tao `Event<UserWindowEvent>`. Window events are filtered to handler's `WindowId`; global events reach all handlers. Hook cleanup unregisters handler. Sources: `packages/desktop/src/hooks.rs:23-41`; `event_handlers.rs:30-69`.

Use desktop-only hooks:

- `use_muda_event_handler` for `muda::MenuEvent`.
- `use_tray_menu_event_handler` for tray menu events.
- `use_tray_icon_event_handler` for tray icon events.
- `use_global_shortcut` for shortcut registration with automatic cleanup.

All are gated to Windows, Linux, or macOS. Sources: `packages/desktop/src/hooks.rs:43-91`, `118-135`.

Create menu through re-exported `muda`, pass with `Config::with_menu` or `WindowConfig::with_menu`, and compare stable item IDs in handler. Borderless window suppresses menu. Source: `examples/08-apis/custom_menu.rs:4-41`; `packages/desktop/src/config.rs:168-176`, `276-290`.

Create tray once in `use_hook` with `init_tray_icon`. It builds and provides tray in context; keep main window `WindowHides` for persistent tray app. Left click shows and focuses all windows when app setting remains enabled. Sources: `examples/08-apis/multiwindow_with_tray_icon.rs:19-31`; `packages/desktop/src/trayicon.rs:26-65`; `app.rs:148-164`.

## Native permissions and platform APIs

Dioxus grants no blanket native permission layer. Rust process can call OS crates, but packaged app still needs OS metadata, entitlements, sandbox declarations, user consent, and signing. Keep permission prompts near operation, handle denial as normal result, and test signed package rather than dev binary.

- macOS: configure entitlements and Info.plist through bundle settings. Source: `packages/cli/src/config/bundle.rs:152-217`.
- Windows: configure signing, WebView2 install mode, fixed runtime, or custom resource manifest. Source: `packages/cli/src/config/bundle.rs:220-340`.
- Linux: desktop features depend on system WebKit/GTK stack; global shortcuts work only on X11 per `DesktopService::create_shortcut`. Source: `packages/desktop/src/desktop_context.rs:292-303`.
- Menus, tray, and global shortcuts use target-gated dependencies. `linux-libxdo` is opt-in for predefined clipboard menu items. Source: `packages/desktop/Cargo.toml:59-71`, `97-107`.
