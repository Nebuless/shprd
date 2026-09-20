# Features, deployment, and security

Source pin: `https://github.com/DioxusLabs/dioxus.git@fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Feature selection

- Shared app enables `dioxus/fullstack` plus one platform. Server build enables `dioxus/server`; browser enables `dioxus/web`; native clients select renderer and point RPC at server.
- `dioxus/fullstack` wires fullstack crate, config macro, serde, and optional web hydration/document. `dioxus/server` wires server support and SSR.
- `dioxus-fullstack` features split `web`, `server`, optional `postcard`, optional `msgpack`, and WebSocket support.
- Gate server-only imports, secrets, filesystem/DB code, and Axum state with `#[cfg(feature = "server")]`. Macro isolates server-function body, not arbitrary surrounding helpers/imports.
- Compile each target independently. Default-feature success does not prove intended graph.

Source evidence: `packages/dioxus/Cargo.toml:44-115`; `packages/fullstack/Cargo.toml:93-115`. Example feature layouts: `examples/07-fullstack/hello-world/Cargo.toml`, `desktop/Cargo.toml`, `auth/Cargo.toml`.

## Middleware boundaries

- `#[middleware(layer)]` applies Tower layer to one generated route. Macro collects stacked attributes and emits `.layer(...)` calls.
- Axum `Router::layer` applies policy at router boundary. Use for CORS, tracing, compression, timeout, body limits, auth context, and request IDs across route groups.
- Order matters. Confirm which layer sees raw request, inserts extensions, and wraps extractor work.
- Server-only extractors execute after route middleware. Use extractors for endpoint-local guarantees and router layers for broad policy.
- Keep SSR and API policy distinct. Document GET, assets, RPC, SSE, and upgrades often need different cache, CSP, CORS, and timeout settings.

Macro evidence: `packages/fullstack-macro/src/lib.rs:206-227`. Example evidence: `examples/07-fullstack/middleware.rs:12-55`.

## Security checklist

Treat every generated server function as public HTTP endpoint.

- Authenticate server-side and authorize requested object/action. Hidden UI, client `cfg`, and typed signature are not access control.
- Validate path/query/body and enforce size, rate, and time limits before expensive work.
- Add CSRF protection for cookie-authenticated mutations. SameSite helps but is not full policy.
- Use explicit CORS origins/methods/headers with credentials. Expose only required response headers.
- Keep secrets out of client bundle, hydration payload, serialized errors, tracing fields, URLs, and HTML.
- Return stable public errors; log private cause with request correlation.
- Reject path traversal and unsafe filenames; cap uploads and serialization expansion.
- Validate WebSocket Origin/session at upgrade; limit frames/connections. Bound SSE/stream producers and stop on disconnect.
- Add CSP and safe output handling for raw HTML/script. Use framework hydration serializer, not handcrafted interpolation.
- Keep authenticated SSR/per-user hydration out of shared ISR/CDN caches unless key and headers prove isolation.
- Trust forwarded headers only from configured proxies. Terminate TLS at trusted edge/service.

Runtime supplies transport, not application authorization. Typed RPC removes boilerplate, not hostile input.

## Redirect policy

- Validate destination. Prefer relative same-origin routes.
- RPC redirect does not automatically drive Dioxus router. Navigate explicitly after action.
- Use 303 after form mutation when next request should be GET; preserve method only deliberately with 307/308.

Runtime evidence: `packages/fullstack/src/payloads/axum_types.rs:37-52`; example caveat: `examples/07-fullstack/redirect.rs:1-49`.

## Deployment topology

- Default server binds `IP` and `PORT`, falling back to `127.0.0.1:8080`.
- CLI bundle puts assets beside executable under `public`; `ServeConfig::new` discovers shell there. Missing shell can produce SSR-only HTML without hydration bootstrap.
- Base path comes from CLI config. Launch router nests app and rewrites root request; verify asset URLs, server-function paths, router prefix, and proxy behavior together.
- Native clients need absolute server URL through `set_server_url` before first call. Same-origin browser deployment avoids most CORS/cookie complexity.
- Proxy must support unbuffered streams, SSE idle duration, WebSocket upgrades, body limits, and correct forwarded scheme/host.
- Add explicit health/readiness routes before SSR fallback. GET fallback can mask missing-route expectations.
- Graceful shutdown, migrations, secrets, observability, backup, and horizontal state belong to hosting architecture, not launch defaults.

Runtime evidence: `packages/fullstack-server/src/launch.rs:85-149,265-290`; `packages/fullstack-server/src/config.rs:42-80`; `packages/fullstack-server/src/server.rs:145-173,376-447`; native URL: `packages/fullstack/src/client.rs:497-514` and `examples/07-fullstack/desktop/src/main.rs:5-9`.
