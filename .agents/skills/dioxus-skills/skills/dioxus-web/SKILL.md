---
name: dioxus-web
description: Builds and diagnoses Dioxus Web apps. Use when working on WASM launch and tooling, DOM mutations and browser events, hydration or streaming hydration, browser history and document APIs, JavaScript eval, web assets, router integration, SSR boundaries, browser testing, deployment, or web-only failures.
metadata:
  invocation: model
---

# Dioxus Web

Local Dioxus source at `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26` is authority. It is 0.8-alpha. Dioxus 0.7 docs linked below explain concepts only; confirm behavior against pinned source before changing code.

## Workflow

1. Classify target as client-only WASM, fullstack hydrated client, or server render. Confirm enabled Cargo features and build target before debugging runtime behavior.
2. Read [`references/launch-renderer.md`](references/launch-renderer.md) for launch selection, root mounting, renderer mutations, browser events, and renderer failure diagnosis.
3. Read matching branch:
   - [`references/hydration-ssr.md`](references/hydration-ssr.md) for hydration, streaming suspense, SSR boundaries, or server/client DOM mismatch.
   - [`references/browser-integration.md`](references/browser-integration.md) for history, router setup, document head, eval, `web-sys`, files, or browser APIs.
   - [`references/assets-toolchain-deployment.md`](references/assets-toolchain-deployment.md) for assets, `dx serve`, `dx build`, WASM tooling, base paths, hosting, tests, or deployment.
4. Reproduce through browser surface. Check browser console, network requests, generated `index.html`, rendered DOM, navigation history, and post-hydration updates as relevant.
5. Tie every diagnosis to one pinned path and symbol from references. Finish when failing browser behavior passes in development and release-shaped build.

## Hard boundaries

- `dioxus_web::run` owns browser client rendering. Server HTML comes from SSR/fullstack crates, then web client may hydrate it. See `packages/web/src/lib.rs::run` and `packages/fullstack-server/src/ssr.rs`.
- Hydration is markerless at this SHA. Diagnose expected DOM shape, browser parser changes, text-node merging, suspense paths, and hydration payloads. Don't look for old hydration comments or `data-node-hydration` markers. See `packages/web/src/hydration/walk.rs::WebsysDom::emit_scope` and `packages/web/src/hydration/cursor.rs::HydrationCursor`.
- Raw JavaScript passed to `document::eval` executes in page context. Treat script text as trusted code and exchange serializable values through `send`, `recv`, or final result. See `packages/web/src/document.rs::WebEvaluator::create`.
- Browser route history needs host fallback to `index.html` for direct path loads. Hash routing avoids that server requirement. See `packages/web/src/history.rs::WebHistory`, `packages/web/src/history.rs::HashHistory`, and `packages/cli/src/serve/server.rs::no_cache`.

## Explanatory docs

- [Dioxus 0.7 Web guide](https://dioxuslabs.com/learn/0.7/guides/platforms/web)
- [Dioxus 0.7 UI escape hatches](https://dioxuslabs.com/learn/0.7/essentials/ui/escape)
- [Dioxus 0.7 web testing](https://dioxuslabs.com/learn/0.7/guides/testing/web)
- [Dioxus 0.7 SSR](https://dioxuslabs.com/learn/0.7/essentials/fullstack/ssr)
- [Dioxus 0.7 HTML streaming](https://dioxuslabs.com/learn/0.7/essentials/fullstack/streaming)
- [Dioxus 0.7 deployment](https://dioxuslabs.com/learn/0.7/tutorial/deploy)

These pages describe stable 0.7 user workflows. Local source is 0.8-alpha and wins on API names, defaults, hydration internals, bundle layout, and edge behavior.
