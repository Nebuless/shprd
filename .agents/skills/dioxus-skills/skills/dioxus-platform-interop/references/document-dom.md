# Document, eval, and DOM

## Portable document surface

- `packages/document/src/lib.rs:document` consumes `Rc<dyn Document>`. Missing renderer context logs an error and returns `NoOpDocument`.
- `packages/document/src/lib.rs:eval` calls `Document::eval` and returns `Eval`.
- `packages/document/src/document.rs:Document` supplies eval, title, and head-element operations. `NoOpDocument` returns `EvalError::Unsupported`; SSR and unsupported native contexts can therefore compile without executing JS.
- `packages/document/src/eval.rs:Eval` is `Copy`. `send` serializes Rust to `serde_json::Value`; `recv<T>` and `join<T>` deserialize. Consumed or dropped evaluators report `EvalError::Finished`.
- Awaiting `Eval` uses `IntoFuture` and joins once. Use `recv` for a stream of `dioxus.send(...)` messages, `send` for `await dioxus.recv()`, and `join` for final JS return value.

Treat this as shared API, not identical transport. Web uses in-process WASM and JS channels. Desktop and mobile webviews use native IPC. LiveView uses its own remote transport. Server and unsupported providers can be no-op.

## Lifecycle

- Start DOM-dependent eval from an event, `use_effect`, or after `onmounted`. Component body runs before mounted DOM is guaranteed and reruns can create duplicate eval tasks.
- Own long-running eval in one component or hook. Define shutdown message or JS completion, then drop it during owner cleanup. A loop blocked on `dioxus.recv()` otherwise has no app-level completion protocol.
- Keep head mutations in `document::Title`, `Meta`, `Script`, `Style`, `Link`, or `Stylesheet` where possible. Renderer implementations queue head changes as effects. The default `Document::create_head_element` notes that creation must happen in an effect to avoid suspended renders.
- Handle `EvalError::Unsupported`, `Finished`, serialization, invalid JS, and communication errors at boundary. Do not assume every renderer executes JS.

## Injection safety

- Eval executes trusted source only. Never format user text, URL data, database content, or remote responses into JS source.
- Send data through `Eval::send`, serialize a prevalidated constant with `serde_json`, or bind it through a typed WASM API. Rust debug formatting is not a general JS escaping contract.
- `dangerous_inner_html` accepts trusted, sanitized HTML only. It bypasses RSX children and opens XSS when fed untrusted markup.
- Minimize desktop webview privileges too. Native Rust access raises impact if page content or navigation can be influenced by an attacker.

## Web Components

- `packages/core-macro/docs/rsx.md` records parser behavior: a tag containing `-` is emitted as an untyped Web Component.
- Quote custom attributes because no generated attribute set exists. Wrap third-party elements in a typed Dioxus component so props, defaults, and event translation have one owner.
- Load element registration code before first use. Use `document::Script`, an `asset!` JS module with static-head options, or app index setup. Treat registration order as explicit, especially during hydration.
- Pass attributes as strings or supported RSX attribute values. Set complex JS properties after mount through `web-sys` or eval. Attribute changes and property changes are not equivalent for many custom elements.
- Browser custom events aren't automatically a typed Dioxus contract. Use web event downcasting where a matching Dioxus event exists, or install one JS listener after mount and relay a structured payload.
- Shadow DOM stays owned by Web Component. Style through documented parts, slots, attributes, or CSS custom properties instead of reaching into private shadow roots.

## Mounted DOM and events

- Prefer cross-platform `MountedData` operations when enough. On web, `dioxus_web::WebEventExt` exposes `try_as_web_event` and panicking `as_web_event`.
- `packages/web/src/events/mounted.rs` maps `MountedData` to `web_sys::Element`. This requires `dioxus-web` `mounted` feature; converter panics when feature is absent.
- Use `try_as_web_event` in reusable code. `as_web_event` assumes web renderer and correct event type.
- Store mounted handles only while node identity remains valid. Conditional rendering, keyed replacement, navigation, and hot reload can replace DOM nodes.
- Avoid mutating nodes Dioxus owns in ways that invalidate virtual DOM assumptions. Canvas state, focus, measurements, observers, and third-party widget roots are safer than replacing managed children.

## Renderer edges

- Web evaluator wraps JS in an async function, calls `dioxus.close()` after source finishes, and converts final result through JSON. `undefined`, invalid UTF-16, circular objects, functions, DOM nodes, and other non-JSON values cannot join successfully.
- Desktop eval routes queries through webview IPC. Closing window or dropping query can end communication. JavaScript garbage collection participates in query cleanup through `FinalizationRegistry`, so cleanup timing is nondeterministic. Treat every send and receive as fallible.
- LiveView JavaScript runs in browser while Rust runs remotely. Keep payloads small and expect latency or disconnects.
- SSR cannot inspect mounted DOM. Defer browser-only behavior until hydration and gate it by target or feature.
- Head insertion has no general deduplication or removal contract. Give repeated imperative insertions stable ownership and idempotence.
