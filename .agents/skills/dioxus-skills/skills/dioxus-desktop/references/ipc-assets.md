# IPC, JavaScript, and assets

Source pin: `DioxusLabs/dioxus@fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Public Rust-JavaScript channel

Use `document::eval(js)`. Desktop document creates `DesktopEvaluator`, backed by window query engine. `Eval::send` sends serializable Rust value to `await dioxus.recv()` in JavaScript. `dioxus.send(value)` feeds `eval.recv::<T>().await`. Awaiting eval returns script result. Communication failures become `EvalError::Communication`. Sources: `packages/desktop/src/document.rs:26-34`, `83-129`; `examples/08-apis/eval.rs:19-40`.

Parse received values into concrete types. Handle `send`, `recv`, and final eval errors separately. One eval instance is one conversation. Don't expose filesystem, shell, token, or unrestricted native dispatch through arbitrary script strings.

Official 0.7 guide calls this `use_eval`; pinned source and example use `document::eval`. Guide explains WebView model and two-way messages but isn't API authority for this SHA.

## Internal IPC

Wry IPC handler receives JSON request body, deserializes `IpcMessage`, and posts `UserWindowEvent::Ipc` to main event loop. Recognized method strings are `initialize`, `query`, `browser_open`, and `user_event`; unknown methods become `Other` and are ignored by launch dispatch. Invalid JSON is silently dropped. This protocol is renderer internals, not public app extension point. Sources: `packages/desktop/src/webview.rs:344-353`; `packages/desktop/src/ipc.rs:51-83`; `packages/desktop/src/launch.rs:134-140`.

Use `document::eval`, custom protocol, DOM events, or typed Rust state for app features. Don't forge internal `IpcMessage` methods.

## Index and assets

WebView loads `dioxus://index.html/`. Built-in protocol order is root index, `__events`, `__file_dialog`, named component asset handler, then `dioxus_asset_resolver::native::serve_asset`. Failed native asset resolution returns HTTP 500 body `Failed to serve asset`. Source: `packages/desktop/src/protocol.rs:30-92`.

Use `asset!("/path")` for bundled local assets. Use `use_asset_handler(name, handler)` when component must answer requests under first path segment. Hook registration replaces same-name handler and removes it on cleanup. Handler must always call responder when it claims request. Sources: `packages/desktop/src/hooks.rs:93-116`; `packages/desktop/src/assets.rs:6-55`.

Use `WindowConfig::with_custom_protocol` for synchronous response or `with_asynchronous_custom_protocol` when response arrives later. Keep responder exactly once. Sources: `packages/desktop/src/config.rs:191-233`; `webview.rs:455-475`.

`with_custom_index` must contain closing `</head>` when adding custom head and always contain closing `</body>`, because loader injection searches those exact strings and panics if absent. Root element ID must match `with_root_name`. Source: `packages/desktop/src/config.rs:241-266`; `packages/desktop/src/protocol.rs:103-131`, `149-180`.

## Navigation and security

Dioxus schemes may load only once. HTTP, HTTPS, and mailto links open through system browser and are blocked in app WebView. Other URLs default allowed, mainly for iframes, unless `with_navigation_handler` returns false. Source: `packages/desktop/src/webview.rs:404-430`; `packages/desktop/src/config.rs:307-312`.

For untrusted content, set navigation handler allowlist. Custom protocol handlers must normalize paths, reject traversal, set content type, and avoid reflecting secrets. Desktop Rust is native even though UI is WebView; browser sandbox assumptions don't protect exposed Rust capabilities.
