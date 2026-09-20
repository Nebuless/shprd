# Fullstack source map

- Repository: `https://github.com/DioxusLabs/dioxus.git`
- Local checkout: `/root/repo/dioxus`
- Exact SHA: `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`
- Explanatory docs: [Dioxus 0.7 fullstack](https://dioxuslabs.com/learn/0.7/essentials/fullstack/)

Source at pin decides behavior. Docs explain intended use. Examples prove composition at that commit. Never promote an example comment into runtime guarantee without implementation anchor.

## Macro-generated contract

- `packages/fullstack-macro/src/lib.rs:89-172`: attribute entry points and `#[server]` defaults.
- `packages/fullstack-macro/src/lib.rs:198-384`: route/function parse, middleware, path/query/body/server-only partition.
- `packages/fullstack-macro/src/lib.rs:385-1924`: generated endpoint constants, client stub, server handler, registration, and compile checks.

## Runtime contract

- `packages/fullstack/src/magic.rs:140-588`: request encoding, response decoding, Axum extraction specialization.
- `packages/fullstack/src/magic.rs:599-852`: Axum success/error response conversion.
- `packages/fullstack/src/request.rs:9-207`: `IntoRequest`, `FromResponse`, response parts, signature diagnostics.
- `packages/fullstack/src/client.rs:20-220,497-514`: URL/query/header/body clients, multipart, server URL.
- `packages/fullstack-server/src/serverfn.rs:12-147`: inventory route and scoped handler execution.
- `packages/fullstack-server/src/server.rs:21-173,208-493`: router extension, registration, assets, SSR fallback, state.
- `packages/fullstack-server/src/launch.rs:37-149,176-217,265-290`: launch, custom serve, bind, upgrades, base path.
- `packages/fullstack-server/src/config.rs:13-224`: shell, streaming mode, ISR, context providers.
- `packages/fullstack-server/src/ssr.rs:31-220,625-717`: request render and document/hydration serialization.
- `packages/fullstack-server/src/streaming.rs:1-150`: suspense streaming protocol.
- `packages/fullstack-core/src/streaming.rs:12-313`: request context, status/header commit boundary.
- `packages/fullstack-core/src/transport.rs:14-230`: positional hydration data.
- `packages/web/src/lib.rs:67-166` and `packages/web/src/hydration/hydrate.rs:1-140`: browser hydration.
- `packages/fullstack/src/payloads/`: forms, query, multipart, files, headers, streams, SSE, WebSockets, and Axum types.
- `packages/dioxus/Cargo.toml:44-115` and `packages/fullstack/Cargo.toml:93-115`: feature graph.

## Example evidence

- `examples/07-fullstack/server_functions.rs`: route syntax, generated RPC constraints, custom response, errors, anonymous endpoints.
- `custom_axum_serve.rs`, `middleware.rs`, `server_state.rs`: Axum composition, layers, state.
- `query_params.rs`, `header_map.rs`, `login_form.rs`, `multipart_form.rs`: request payload/extractor patterns.
- `streaming_file_upload.rs`, `streaming.rs`, `server_sent_events.rs`, `websocket.rs`: long-lived and binary transports.
- `redirect.rs`, `handling_errors.rs`, `full_request_access.rs`: control flow, status, request context.
- `router/src/main.rs`, `ssr-only/src/main.rs`: SSR, ISR, and no-hydration mode.

## Anchor maintenance

Before using this skill against another revision:

1. Compare changed files above against new HEAD.
2. Re-resolve line anchors and behavior, not line numbers alone.
3. Reconcile docs and examples under source precedence.
4. Update pin and all affected claims together.
