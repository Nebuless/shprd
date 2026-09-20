# Rendering and navigation

Authority: Dioxus commit `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`. Official explanation: [navigation](https://dioxuslabs.com/learn/0.7/essentials/router/navigation/) and [layouts](https://dioxuslabs.com/learn/0.7/essentials/router/layouts/).

## Router and outlet depth

`Router::<Route>` installs one `RouterContext`, installs `OutletContext<Route>` at level zero, then renders root `Outlet`. Each `Outlet` reads current level, provides level plus one to descendants, and calls current route's generated `render(level)`. Derived rendering emits active layouts first, then leaf component. Missing outlet means deeper content never renders. Extra outlet past leaf renders empty output.

Router config is initialization-only. `RouterProps::PartialEq` always returns true, so changing config callback or initial URL through props does not rebuild router. Construct stable config before first render.

```rust
#[derive(Clone, PartialEq, Routable)]
#[rustfmt::skip]
enum Route {
    #[nest("/team/:team_id")]
        #[layout(TeamShell)]
            #[route("/")]
            TeamHome { team_id: u64 },
            #[route("/member/:member_id")]
            Member { team_id: u64, member_id: u64 },
        #[end_layout]
    #[end_nest]
}

#[component]
fn TeamShell(team_id: u64) -> Element {
    rsx! {
        nav { Link { to: Route::TeamHome { team_id }, "Team" } }
        Outlet::<Route> {}
    }
}
```

Layouts retain component state while navigating between leaves at same layout level. Pinned regression test `layout_retains_state_after_navigation` proves one layout instance survives route change.

## Link

`Link` emits an HTML anchor and asks router to handle unmodified primary-button internal clicks. Modified clicks, non-primary clicks, and `new_tab` fall through to platform anchor behavior. External links also fall through unless `onclick_only` changes default handling. Internal `href` includes history prefix. External URL does not.

Active state is exact string equality against full current route. Matching adds `active_class` and `aria-current="page"`; parent routes are not active for descendants. External links default `rel="noopener noreferrer"`. `new_tab` adds `target="_blank"`.

`onclick_only: true` with an `onclick` handler suppresses ordinary link navigation and calls handler after preventing default for eligible clicks. With default `onclick_only: false`, internal navigation happens before custom handler. Outside router, `Link` panics in debug and returns empty node in release.

`Link` exists only with router `html` feature. Non-HTML targets can use buttons plus `navigator()` or `router()`.

## Typed navigation

Prefer:

```rust
let nav = navigator();
nav.push(Route::Post { id: 7 });
nav.replace(Route::Home {});
if nav.can_go_back() {
    nav.go_back();
}
```

`push` adds current location to back history. `replace` does not. `go_back` and `go_forward` silently do nothing when provider cannot move. `navigator()` and `router()` do not subscribe caller to route updates. `use_route::<Route>()` reads current route and subscribes through router internals. `use_navigator()` captures `Navigator` once. Deprecated `use_router()` should be replaced by `router()` or `use_route()`.

`NavigationTarget::Internal` goes through history provider. `NavigationTarget::External` calls `History::external`; failure becomes `ExternalNavigationFailure`, stored router error, and rendered only at root outlet through configured `failure_external_navigation` component. `Link` can still expose external URL directly as anchor even when provider cannot perform programmatic external navigation.

## History and platform differences

Renderers usually install history. `HistoryProvider` allows explicit `Rc<dyn History>` and initializes it once. Use `MemoryHistory` for SSR and deterministic tests. Base prefix is prepended only to internal link `href`s. Current route remains provider's route string.

Pinned `dioxus-history` crate contains `MemoryHistory` and generic `History` context. Web and LiveView histories are supplied outside `packages/history/src` by renderer integrations. Router itself detects LiveView through `History::include_prevent_default` and emits inline click prevention because server-side event handling cannot call browser `preventDefault` in time.

Version mismatch: official navigation page says router provides "two" providers, then lists `MemoryHistory`, `LiveviewHistory`, and `WebHistory`, and its override example does not override history. Treat that section as stale prose. Use renderer setup or explicit `HistoryProvider`; inspect platform integration source when provider choice matters.

Browser history has platform limits. `can_go_back` and `can_go_forward` depend on provider. Native apps may need `GoBackButton` and `GoForwardButton`, both gated behind `html` feature despite being useful in HTML-backed desktop/mobile renderers.

## Update callbacks and guards

`RouterConfig::on_update` runs after history changes and before subscribers update. Returning target performs one `replace`, not another callback cycle. Use it for synchronous canonicalization or local route gates:

```rust
Router::<Route> {
    config: || RouterConfig::default().on_update(|state| {
        let route = state.current();
        matches!(route, Route::LegacyHome {})
            .then_some(NavigationTarget::Internal(Route::Home {}))
    })
}
```

Callback runs for `push`, `replace`, back, and forward through `change_route`. It is not async. External-navigation failure returns before callback. Keep callback pure and bounded; returning target replaces once, then subscribers update.
