# Route model

Authority: Dioxus commit `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`. Official explanation: [routing overview](https://dioxuslabs.com/learn/0.7/essentials/router/) and [defining routes](https://dioxuslabs.com/learn/0.7/essentials/router/routes/).

## `Routable` contract

`Routable` requires `FromStr`, `Display`, `Clone`, and `'static`; it supplies `SITE_MAP` and `render(level)`. Derive it on one route enum. Each variant must use named fields and have `#[route(...)]` or `#[child(...)]`. By default variant `Post` renders component `Post`; `#[route("/post", PostPage)]` selects another component.

```rust
use dioxus::prelude::*;

#[derive(Clone, Debug, PartialEq, Routable)]
enum Route {
    #[route("/")]
    Home {},
    #[route("/post/:id")]
    Post { id: u64 },
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

#[component]
fn Home() -> Element { rsx! { "Home" } }

#[component]
fn Post(id: u64) -> Element { rsx! { "Post {id}" } }

#[component]
fn NotFound(segments: Vec<String>) -> Element {
    rsx! { "No page at /{segments.join(\"/\")}" }
}
```

Derived code implements URL parsing, URL display, component rendering, generated parse-error enums, and `SITE_MAP`. `Routable::parent`, `is_child_of`, `static_routes`, and `flatten_site_map` are path helpers, not authorization checks.

## Segments and bounds

| Syntax | Input trait | Output trait | Failure behavior |
|---|---|---|---|
| `/fixed` | exact text | built in | candidate fails on mismatch |
| `/:id` | `FromRouteSegment`, blanket `FromStr` | `Display` | candidate fails on parse error |
| `/:..tail` | `FromRouteSegments` | `ToRouteSegments` | candidate fails on parse error |
| `?:page&:filter` | `FromQueryArgument + Default` | `ToQueryArgument` | missing or invalid value becomes default |
| `?:..query` | `FromQuery` | `Display` | parser owns whole decoded query |
| `#:fragment` | `FromHashFragment` | `Display` | blanket `FromStr + Default` logs and defaults |

Use `Option<T>` for optional named query values. Missing `Option<T>` becomes `None`; malformed input also becomes `None`. Named query parsing collects pairs into a `HashMap`, so duplicate keys collapse and pair order carries no meaning. Pairs without `=` are ignored. Query names come from Rust field names.

Path, query, and hash values are percent-decoded before parsing. Display percent-encodes dynamic path values, query output, and hash output with separate encode sets. A custom full-query parser receives decoded text, not raw URL bytes.

Catch-all must be final and is forbidden in `#[nest]`. Route and nest strings must start with `/`. Query and hash syntax belongs on routes or redirects, not nests. Trailing slash is ignored during parsing. Input without leading slash fails before candidates are attempted.

## Matching order

Parser tree favors static segments over dynamic segments over catch-all segments. Definition order breaks ties. Query and hash parsing occurs after path matching for a candidate. Put broad catch-all not-found route last for reader clarity, though specificity keeps static and dynamic routes ahead.

Source mismatch: derive macro documentation says query routes are considered before static routes. Generated parse tree and current official 0.7 route docs describe path specificity as static, dynamic, catch-all, with query and hash attached to candidates. Trust `route_tree::ParseRouteTree` at pinned source.

## Nests, layouts, and child routers

`#[nest("/team/:team_id")]` prefixes following routes until `#[end_nest]` or enum end. Every child variant needs matching parent dynamic fields. Layout components receive only dynamic fields from active nests. Route-local dynamic fields can be read with `use_route::<Route>()` inside a layout.

`#[layout(Shell)]` wraps following routes until `#[end_layout]` or enum end. `#[layout(!Shell)]` excludes an already active layout for one variant. Each active layout must render `Outlet::<Route> {}` to expose the next level.

`#[child("/admin")] Admin { child: AdminRoute }` mounts another `Routable` enum. Field must be named `child` or marked `#[child]`. Derived mapping converts child URLs to root URLs for links and navigation, and includes child `SITE_MAP` under `SegmentType::Child`. Pinned `ChildRouteMapping` supports a simple static prefix; use nests in one route enum when prefix needs parameters.

## Redirects and not-found

`#[redirect("/old/:id", |id: u64| Route::Post { id })]` is parse-time canonicalization. Redirect closure arguments must be typed identifiers matching path, query, hash, and active-nest parameters. Result is route value; parsing does not create a history entry by itself. Redirect order follows endpoint order among otherwise matching candidates.

There is no dedicated not-found component in pinned router source. Define final catch-all route. If malformed typed dynamic input should show 404, ensure catch-all accepts remaining path. If no variant matches, `Route::from_str` returns `RouteParseError`; `RouterContext::current` throws `ParseRouteError`, then attempts to parse `/` as fallback and panics if `/` also has no valid route.

## Edge cases

- `Routable::parent` strips query and hash, removes one path segment, then reparses. It returns `None` if no enum route matches that parent path.
- `is_child_of` compares formatted path segments after stripping query, hash, and trailing slash. Same route is not its own child.
- Fields not referenced by route, nest, query, or hash syntax are filled with `Default::default()` during parsing.
- Empty optional query output can leave a trailing `?` in some all-empty forms. Assert exact display strings your app relies on.
- External absolute URLs and internal typed routes are distinct `NavigationTarget` variants. String targets are less safe because classification depends on parseability and router context.
