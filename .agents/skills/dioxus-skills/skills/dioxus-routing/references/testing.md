# Routing tests

Run against source SHA `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Parse and display test

Put pure route tests beside route enum. Assert both directions because derive generates independent `FromStr` and `Display` code.

```rust
#[test]
fn route_round_trips_params_query_and_hash() {
    let route = Route::Search {
        term: "rust ui".to_string(),
        page: Some(2),
        section: "results".to_string(),
    };

    let encoded = route.to_string();
    assert_eq!(encoded, "/search/rust%20ui?page=2#results");
    assert_eq!(encoded.parse::<Route>().unwrap(), route);
}
```

Cover:

- static, dynamic, catch-all, named query, full query, and hash forms used by app
- custom parser rejection followed by next candidate or catch-all
- missing and malformed query defaults
- percent-encoded `/`, `?`, `#`, spaces, and non-ASCII text
- trailing slash equivalence and required leading slash
- redirect result and definition-order ties
- child-route query and hash round trip
- `parent`, `is_child_of`, and `static_routes` only when app depends on them

## Render test with real history

Use real `MemoryHistory`, `VirtualDom`, and SSR renderer. Avoid mocking router context.

```rust
use dioxus::prelude::*;
use dioxus_history::{History, MemoryHistory};
use dioxus_router::components::HistoryProvider;
use std::rc::Rc;

#[component]
fn TestApp() -> Element {
    rsx! {
        HistoryProvider {
            history: |_| Rc::new(
                MemoryHistory::with_initial_path("/team/7/member/9")
            ) as Rc<dyn History>,
            Router::<Route> {}
        }
    }
}

#[test]
fn nested_layout_renders_leaf() {
    let mut vdom = VirtualDom::new(TestApp);
    vdom.rebuild_in_place();
    assert_eq!(
        dioxus_ssr::render(&vdom),
        "<nav><a href=\"/team/7\">Team</a></nav><main>Member 9</main>"
    );
}
```

Assert observable HTML: layout order, leaf content, internal prefixed `href`, external `rel`, active class, `aria-current`, and root external-error rendering. Pinned tests under `packages/router/tests/via_ssr/` are executable templates.

## Navigation test

Drive route change through `router().push`, `replace`, or link event in a real `VirtualDom`. Render after queued work. Check current route and visible page. For history semantics, use `MemoryHistory` and test back/forward separately.

Guard tests need distinct protected, login, and public routes. Prove callback replacement does not add a second history entry. For async auth layout, test loading, allowed, and denied outcomes. Server authorization needs HTTP-level tests independent from client guard.

## Compile-time cases

Macro constraints are best tested with compile-fail fixtures when maintaining router itself. Application tests usually need only compile coverage plus runtime tests. Errors worth recognizing:

- route or nest missing leading `/`
- missing variant field for dynamic/query/hash name
- catch-all followed by another path segment
- catch-all inside nest
- child variant without named child field
- excluding layout not currently defined
- redirect closure argument lacking typed identifier pattern

## Pinned executable evidence

- `packages/router/tests/parsing.rs`: trailing slash, query defaults, optional query, percent encoding, child query/hash round trips.
- `packages/router/tests/via_ssr/link.rs`: internal/external links, prefixes, active state, new tabs, child links, hash links.
- `packages/router/tests/via_ssr/outlet.rs`: static and dynamic nests with layouts.
- `packages/router/tests/via_ssr/navigation.rs`: layout state retention during navigation.
- `packages/router/tests/via_ssr/redirect.rs`: redirect ordering.
- `packages/router/tests/via_ssr/child_outlet.rs`: child-router rendering.
- `packages/router/tests/site_map.rs` and `parent.rs`: generated site map and hierarchy helpers.

Run focused upstream proof from `/root/repo/dioxus`:

```sh
cargo test -p dioxus-router --test parsing
cargo test -p dioxus-router --test via_ssr
cargo test -p dioxus-router --test site_map --test parent
```

If local SHA differs, these commands test different behavior. Label result version-mismatched rather than claiming pinned proof.
