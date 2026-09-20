# Desktop, native windows, and Android app access

## Desktop renderer

- `dioxus::desktop::use_window()` is hook-based `DesktopContext`; `dioxus::desktop::window()` is imperative context lookup and panics outside Dioxus app context.
- `DesktopContext` is `Rc<DesktopService>` and dereferences to window state. Keep it on owning event-loop thread. It isn't a generic `Send + Sync` app service.
- Desktop has no Dioxus-specific raw-handle helper at this pin. Its window context dereferences to Tao `Window`, and `tao` is reexported; use raw-window-handle traits from that native window only when needed.
- Use `DesktopContext` methods for title, drag, maximize, fullscreen, close, zoom, print, devtools, child windows, shortcuts, Wry event handlers, and asset handlers.
- `new_window` returns `PendingDesktopWindow`. Creation is async because WebView2 forbids nested webview creation from callback. Await before control; use `try_resolve` when cancellation matters.
- Avoid strong reference cycles among windows. Source warns cycles can leak. Use weak handles for callbacks that outlive one window.

## Webview IPC

- App-facing JS exchange is `document::eval` plus `Eval::send`, `recv`, and `join`. This is public Dioxus API.
- `packages/desktop/src/js/native_eval.js`, `query.rs`, `ipc.rs`, `webview.rs`, and `launch.rs` describe internal query transport through `window.ipc.postMessage`. Treat protocol shape and reserved endpoints as internal.
- Do not call `window.ipc` directly or reuse internal `query` messages from app code. They can collide with renderer initialization, event transport, file dialogs, and query cleanup.
- Internal IPC decoding silently drops malformed JSON, unknown methods, invalid query results, and unknown window IDs. This is implementation evidence, not a validation model for custom IPC.
- If custom webview IPC is unavoidable, build it through public Wry configuration or a separate named protocol with strict message parsing, origin checks, capability checks, size limits, and explicit response IDs. Confirm chosen hook is public at pinned source.
- Desktop custom index must retain expected head, body, root, and loader requirements. Source injection searches closing `</head>` and `</body>` and can panic when absent.

## Asset handlers

- `use_asset_handler(name, handler)` registers component-scoped desktop asset route and removes it during hook cleanup.
- `DesktopService::register_asset_handler` registers imperatively; caller owns removal. Names are global by name, not isolated by scope.
- Validate route suffix and content type. Reject traversal and avoid exposing arbitrary filesystem paths.
- Always respond exactly once through `RequestAsyncResponder`, including errors.

## Dioxus Native

- Dioxus Native is separate renderer, not desktop webview mode. `packages/native/src/lib.rs` exports `use_window() -> Arc<dyn Window>` and `use_raw_window_handle() -> RawWindowHandle`.
- `use_raw_window_handle` reads current `winit` window handle and unwraps it. Call only inside active Native renderer scope.
- Raw handle is borrowed capability represented without ownership. Use it during valid window lifetime, on required thread, and never cache past window destruction or recreation.
- Prefer `use_window` and Winit safe APIs. Reach for raw handle only when downstream graphics or OS API requires it.
- Backend variants differ by OS. Match `RawWindowHandle` exhaustively and compile each target-specific branch.

## Android app access

Two distinct APIs exist at pinned source:

- Dioxus Native: `dioxus_native::set_android_app(AndroidApp)` and `current_android_app()`. Getter panics before setup. This belongs to Native renderer startup.
- Manganis mobile bridge: `manganis::android::with_activity`, backed by `ndk_context`, caches `JavaVM` and a JNI `GlobalRef`, attaches current thread, and returns `Option<R>`.

Do not conflate them or invent `dioxus::android()` API. For webview mobile apps using Manganis FFI, generated Kotlin constructors and calls use `with_activity` internally.

## Native lifecycle

- Android Activity can be recreated. Pinned `with_activity` uses a process-wide `OnceLock<GlobalRef>` and offers no refresh API. Treat configuration-change freshness as known gap; verify against target lifecycle before depending on current Activity identity.
- JNI local references live for attached frame; promote only when long-lived ownership is needed. Clear Java exceptions before later JNI calls.
- iOS UIKit work must run on main thread. Desktop source asserts main thread for `push_view` and `pop_view`; retained Objective-C references own their object lifetime.
- Window close, suspend, resume, surface loss, and device scale changes can invalidate native resources. Couple resource lifetime to window or activity event, not component render count.
