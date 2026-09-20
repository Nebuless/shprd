---
name: dioxus-fullstack
description: >-
  Builds and repairs Dioxus 0.7 fullstack apps: server functions, typed RPC,
  Axum routing, SSR and hydration, payloads, streams, middleware, security,
  and deployment. Use when code contains `#[server]`, `#[get]`/`#[post]`,
  `dioxus::serve`, `ServeConfig`, `FullstackContext`, `Streaming`,
  `ServerEvents`, or `Websocket`.
metadata:
  invocation: model
---

# Dioxus fullstack

1. Verify `/root/repo/dioxus` is at `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`. Treat local source as authority. Use [Dioxus 0.7 fullstack guide](https://dioxuslabs.com/learn/0.7/essentials/fullstack/) for explanation and local `examples/07-fullstack` for composition, not runtime contracts.
2. Identify build side and request path: web client, native client, server function, or SSR document request. Confirm active Cargo features before changes.
3. For endpoint signatures, generated client/server behavior, registration, errors, or custom transports, read [`references/server-functions.md`](references/server-functions.md).
4. For `dioxus::launch`, custom Axum routers, SSR requests, suspense streaming, hydration data, status, or headers, read [`references/ssr-axum-hydration.md`](references/ssr-axum-hydration.md).
5. For forms, multipart, files, headers, query, streams, SSE, or WebSockets, read [`references/payloads-realtime.md`](references/payloads-realtime.md).
6. For feature selection, cross-origin clients, middleware, redirects, auth, limits, reverse proxies, or deployment, read [`references/deployment-security.md`](references/deployment-security.md).
7. Test both halves of changed contracts. Compile server and client feature sets separately; exercise real HTTP, streaming, or upgrade path. For SSR changes, load rendered HTML and hydrate it.
8. Recheck claims against [`references/source-map.md`](references/source-map.md). Finish only when macro intent, runtime behavior, and example usage remain distinct.
