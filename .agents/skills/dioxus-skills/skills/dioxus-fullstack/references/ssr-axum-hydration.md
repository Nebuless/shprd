# SSR, Axum, and hydration

Source pin: `https://github.com/DioxusLabs/dioxus.git@fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Launch and router choices

- `dioxus::launch(app)` selects fullstack server or client launch from features. Server launch creates Tokio runtime, `ServeConfig`, Dioxus Axum router, base-path nesting, logger, and bind loop.
- `dioxus::server::router(app)` returns Axum `Router` with server functions, static assets, SSR fallback, base path, and dev integration.
- `dioxus::serve(|| async { Ok(router) })` owns runtime, bind, and hot reload while allowing custom Axum routes/layers.
- `serve_dioxus_application` mounts server functions, assets, and GET SSR fallback. `serve_api_application` omits assets. `register_server_functions` supports headless API composition.
- For full manual ownership, bind listener and call `axum::serve`; compose `DioxusRouterExt` pieces deliberately.

Runtime evidence: `packages/fullstack-server/src/launch.rs:37-149,265-290`; `packages/fullstack-server/src/server.rs:21-173`. Example evidence: `examples/07-fullstack/custom_axum_serve.rs:24-39`.

## SSR request lifecycle

1. Explicit Axum routes and server functions match before fallback.
2. GET fallback passes request and `FullstackState` into `render_handler`.
3. Renderer creates per-request VDOM, `FullstackContext`, history/base path, document provider, and hydration context.
4. Initial render resolves according to `ServeConfig` streaming mode.
5. Response status and headers are taken before initial chunk commits.
6. Document shell receives head, main HTML, hydration data, bootstrap assets, and optional later suspense chunks.

Runtime evidence: `packages/fullstack-server/src/server.rs:154-173,208-260`; `packages/fullstack-server/src/ssr.rs:31-220,625-717`; `packages/fullstack-core/src/streaming.rs:57-92,173-250`.

## Hydration contract

- Fullstack feature enables web hydration when web renderer is selected. Server emits `HydrationContext`; client reads `window.initial_dioxus_hydration_data`, rebuilds VDOM without DOM mutations, then binds existing SSR DOM.
- Serialized entries are positional. Server and client must create them in same order and render structurally compatible trees.
- Server futures, loaders, and cached values can serialize resolved or pending slots. Missing post-hydration data can cause client work to run.
- Hydration mismatch means output diverged: feature/cfg difference, nondeterminism, browser-only first-render branch, changed order, invalid HTML normalization, or version skew.
- SSR-only builds omit client bundle and interactivity. Signals render server-side but do not become browser-reactive.

Runtime evidence: `packages/fullstack-core/src/transport.rs:14-179`; `packages/fullstack-server/src/ssr.rs:674-706`; `packages/web/src/lib.rs:67-166`; `packages/web/src/hydration/hydrate.rs:1-140`. Example evidence: `examples/07-fullstack/ssr-only/src/main.rs:1-59`.

## Suspense and streaming boundaries

- Default `StreamingMode::Disabled` waits for server futures before hydration.
- Out-of-order mode sends shell, then resolved suspense boundary payloads. Client maps streamed IDs to mounted boundaries and hydrates each chunk.
- `commit_initial_chunk()` ends ability to change HTTP status or headers. Router can commit when suspense above router resolves; custom layouts may commit explicitly.
- Head mutations after streaming begins may miss initial head without JavaScript.
- Make request futures cancel-safe. Client disconnect can end response while detached backend work continues.

Runtime evidence: `packages/fullstack-server/src/config.rs:13-32`; `packages/fullstack-server/src/streaming.rs:1-150`; `packages/fullstack-core/src/streaming.rs:77-92,195-275`.

## Request, status, and headers

- Use server-only Axum extractors when request data belongs to endpoint.
- Use `FullstackContext::extract` during SSR only when rendering needs metadata. Outside request scope it creates synthetic empty-body request; never treat that as authenticated context.
- Set status and response headers before initial commit. Later calls cannot rewrite sent bytes.
- Return `HttpError` or known errors from loaders to drive SSR status. Unknown captured errors default to 500.
- Never branch initial client/server tree on secret-only state unless hydration gets equivalent public state.

Runtime evidence: `packages/fullstack-core/src/streaming.rs:105-170,173-239,242-338`. Example evidence: `examples/07-fullstack/full_request_access.rs`; `examples/07-fullstack/ssr-only/src/main.rs:38-58`.

## `ServeConfig`

- `ServeConfig::new` discovers bundled `public/index.html`; missing public path/index falls back to SSR-only shell without JS/WASM bootstrap.
- `incremental` enables rendered-page caching. Cache user-specific output only when key and cache policy isolate users.
- `context_provider` factories may run many times. Return cheap clones or handles, not one-time resources.
- Out-of-order streaming changes response timing and proxy behavior; test through production proxy/CDN.

Runtime evidence: `packages/fullstack-server/src/config.rs:13-112,114-224`; cache implementation: `packages/fullstack-server/src/isrg/`.
