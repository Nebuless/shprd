# Server functions and generated RPC

Source pin: `https://github.com/DioxusLabs/dioxus.git@fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Endpoint form

- Prefer `#[get("/path")]`, `#[post("/path")]`, `#[put]`, `#[delete]`, or `#[patch]` for stable public HTTP routes. Route strings declare path captures and query bindings.
- Use bare `#[server]` for app-internal RPC. It defaults to POST plus `/api`; unnamed endpoint identity derives from source identity and signature. Treat URL as unstable across builds, especially for shipped native clients.
- `#[server(prefix = ..., endpoint = ...)]` remains supported. Method attributes give new code clearer HTTP semantics.
- Every server function returns `Result<T, E>`. Enforce authorization and validation in server body even when UI constrains input.

Macro evidence: `packages/fullstack-macro/src/lib.rs:89-147` maps `#[server]` to POST and `/api`; `:149-172` defines method attributes; `:198-347` parses function, server-only arguments, body fields, and middleware. Example evidence: `examples/07-fullstack/server_functions.rs:188-293`.

## Generated halves

1. Macro parses route, path/query parameters, ordinary arguments, and extra server-only extractors.
2. Server build retains implementation, generates Axum handler, applies declared layers, and submits route metadata to inventory.
3. Client build generates same-signature stub. It builds URL, encodes caller arguments, sends request, and decodes response.
4. Router registration collects inventory entries and mounts unique method/path pairs.

Same Rust call syntax hides HTTP, not local execution. Network errors, status, serialization, CORS, cookies, and version skew still apply.

Macro evidence: `packages/fullstack-macro/src/lib.rs:250-384` computes body/query/path and endpoint; generated branches continue in same file. Runtime evidence: `packages/fullstack-server/src/serverfn.rs:12-147` owns metadata and handler execution; `packages/fullstack-server/src/server.rs:127-173` mounts functions.

## Argument transport

- Multiple ordinary serializable arguments become one generated JSON object. Field names derive from function bindings.
- One transport type can use `IntoRequest` on client and Axum `FromRequest` on server. Use for `Form<T>`, multipart, streams, files, upgrades, or custom bodies.
- Route path/query arguments enter URL, not JSON body.
- Arguments declared in macro after route, such as `headers: HeaderMap`, are server-only extractors. Client signature omits them; server builds them from request.
- Body-consuming extractor must be last. Multipart and streaming bodies cannot be consumed twice.

Runtime evidence: `packages/fullstack/src/magic.rs:140-211` selects JSON versus `IntoRequest`; `:492-588` extracts request parts before body. Diagnostics: `packages/fullstack/src/request.rs:165-207`.

## Response and error transport

- Serializable `Ok(T)` becomes JSON status 200.
- Dioxus `FromResponse` plus Axum `IntoResponse` owns raw HTTP conversion for redirects, files, streams, SSE, WebSockets, and custom response pairs.
- Typed `E` crossing RPC must convert from `ServerFnError` and serialize/deserialize. Implement `AsStatusCode` when status reflects domain error.
- `anyhow::Error`/Dioxus `Result` loses concrete error type on client. Keep internal chains and secrets out of display text.
- `StatusCode` preserves status but loses detail. `HttpError` preserves status and optional public message.
- Client decoder prefers specialized `FromResponse`, then JSON. Non-success JSON errors become `ServerFnError::ServerError`; malformed error bodies fall back to text or decode failure.

Runtime evidence: `packages/fullstack/src/magic.rs:214-483` decodes results; `:599-747` builds Axum responses. Example evidence: `examples/07-fullstack/server_functions.rs:23-65,231-269`.

## Registration and state

- `dioxus::launch` and `dioxus::server::router(app)` auto-register inventory functions.
- Duplicate method/path entries are deduplicated. Never rely on link order to select one.
- Request extensions and shared state belong in Axum layers/extensions or `ServeConfig::context_provider`.
- State-dependent extractors must be satisfiable from `FullstackContext`; verify concrete router composition.
- `FullstackContext::current()` exists only during SSR or server-function execution.

Runtime evidence: `packages/fullstack-server/src/server.rs:127-173`; `packages/fullstack-core/src/streaming.rs:12-30,118-170`. Example evidence: `examples/07-fullstack/server_state.rs:98-160`.

## Debug RPC

1. Compile server and client platform features. Signature can satisfy one half only.
2. Confirm method/path in registration logs.
3. Inspect request URL, `Content-Type`, body, response status, and response `Content-Type`.
4. Classify failure: registration, request encoding, Axum rejection, handler error, response encoding, or client decode.
5. Native clients call `dioxus::fullstack::set_server_url` before first request. Browser clients normally use same origin.
6. Lock stable paths for independently deployed clients; test old-client/new-server compatibility when signatures change.
