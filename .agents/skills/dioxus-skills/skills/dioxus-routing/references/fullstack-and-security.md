# Fullstack, SSR, and security

Authority: Dioxus commit `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## SSR and hydration

Server rendering needs route-aware history. Wrap router in `HistoryProvider` with `MemoryHistory::with_initial_path(request_uri)` when renderer has not already provided request history. Render same route and prefix on server and client to avoid hydration disagreement.

```rust
use dioxus_history::{History, MemoryHistory};
use dioxus_router::components::HistoryProvider;
use std::rc::Rc;

#[component]
fn ServerApp(path: String) -> Element {
    rsx! {
        HistoryProvider {
            history: move |_| Rc::new(
                MemoryHistory::with_initial_path(path.clone())
            ) as Rc<dyn History>,
            Router::<Route> {}
        }
    }
}
```

Pinned SSR tests build `VirtualDom`, install `MemoryHistory`, rebuild, then call `dioxus_ssr::render`. This is executable proof for `Link`, `Outlet`, redirects, child routers, and navigation state.

With router `streaming` feature, `Router` registers `use_after_suspense_resolved` and calls `dioxus_fullstack_core::commit_initial_chunk()`. Route components that suspend can delay initial chunk. With `wasm-split`, derived leaf rendering uses a lazy loader and suspends until route bundle loads; place a suspense boundary above outlet. These are separate features.

## Fullstack integration

Router maps application URLs to components. Server endpoint routing remains owned by fullstack/Axum integration. Keep browser-visible base prefix, server mount path, static assets, and request history aligned. Typed route display yields app-relative path; history prefix supplies deployment subpath to internal links.

Fullstack server functions still need server-side identity and authorization checks. Client route state can shape UI, but it cannot protect data or mutation endpoints.

## Authentication and authorization guards

Choose guard by job:

| Job | Placement |
|---|---|
| hide or replace local page while auth state loads | layout or route component |
| synchronously reroute known unauthenticated state | `RouterConfig::on_update` returning login route |
| preserve intended destination | encode return target in typed query or app state, then validate it |
| protect server-rendered data | server loader, server function, or request middleware |
| reject unauthorized mutation | server function or endpoint before mutation |

Avoid async work inside `on_update`; callback API is synchronous. An auth layout can read reactive auth state, render pending UI, and call `navigator().replace(...)` from an effect once state resolves. Keep hook order stable.

Never trust a client-only guard. Treat return URLs as untrusted. Prefer internal typed route values. If accepting string return URLs, parse them as app route and reject absolute or protocol-relative URLs to prevent open redirects.

## Redirect classes

- `#[redirect]` canonicalizes during route parsing. Good for old internal paths and parameter remapping.
- `RouterConfig::on_update` replaces location after navigation. Good for synchronous policy or canonical state.
- `navigator().replace` changes client history from component logic. Good after async state resolves.
- HTTP 3xx happens before Dioxus rendering. Good for canonical host, login enforcement, and status-correct SSR redirects.

Parse-time and client redirects do not imply HTTP 3xx status. Use server response control when crawlers, caches, or clients need redirect semantics.

## Not-found and error boundaries

Catch-all route provides app 404 UI, but SSR HTTP status must be set by server integration. A rendered `NotFound` component alone commonly returns HTTP 200. Determine route match before final response or communicate status through fullstack response context.

Route parse failure calls `dioxus_core::throw_error(ParseRouteError)` inside `RouterContext::current`, then falls back to `/`. Put a valid root route in enum. Use an error boundary for exceptional parse state; use catch-all for normal unknown paths.

External navigation failures are different. Router stores `ExternalNavigationFailure` and root outlet renders `failure_external_navigation`. Clear with `RouterContext::clear_error()` after recovery.

## Platform checklist

- Web: ensure server fallback serves app for client routes; use browser history integration supplied by renderer; preserve base path.
- Desktop/mobile: no browser address bar guarantee; use renderer history and explicit navigation UI.
- LiveView: router emits JS `preventDefault` for eligible link clicks because event crosses network.
- SSR/static generation: enumerate `Routable::static_routes()` only for paths without dynamic, query, hash, or catch-all data; generate dynamic routes separately.
- Non-HTML build: `Router`, `Outlet`, navigator, and route model remain; `Link`, default external error UI, and history buttons are feature-gated out.
