# Source map and version notes

## Pin

- Repository: `https://github.com/DioxusLabs/dioxus.git`
- Local checkout: `/root/repo/dioxus`
- Exact SHA: `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`
- Pinned URL root: `https://github.com/DioxusLabs/dioxus/blob/fda3dc9c2b10ddf4417edcbb98caa9613ac92d26/`

## Symbols

| Topic | Exact source symbols |
|---|---|
| route contract | `packages/router/src/routable.rs`: `Routable`, `RouteParseError`, `SiteMapSegment`, `SegmentType`, `FromRouteSegment`, `FromRouteSegments`, `ToRouteSegments`, `FromQuery`, `FromQueryArgument`, `ToQueryArgument`, `FromHashFragment` |
| derive entry | `packages/router-macro/src/lib.rs`: `routable`, `RouteEnum::parse`, `RouteEnum::parse_impl`, `RouteEnum::routable_impl`, `RouteEnum::impl_display` |
| route attributes | `packages/router-macro/src/route.rs`: `Route::parse`, `Route::construct`, `Route::routable_match`, `RouteType` |
| path matching | `packages/router-macro/src/segment.rs`: `parse_route_segments`, `RouteSegment::try_parse`, `RouteSegment::write_segment` |
| query | `packages/router-macro/src/query.rs`: `QuerySegment`, `FullQuerySegment`, `QueryArgument` parse/write methods |
| hash | `packages/router-macro/src/hash.rs`: `HashFragment` parse/write methods |
| nests/layouts | `packages/router-macro/src/nest.rs`: `Nest::parse`; `layout.rs`: `Layout::routable_match`; `route_tree.rs`: `ParseRouteTree` |
| child routers | `packages/router-macro/src/route.rs`: `RouteType::Child`, child render arm; `packages/router/src/components/child_router.rs`: `ChildRouteMapping`, `ChildRouter` |
| redirects | `packages/router-macro/src/redirect.rs`: `Redirect::parse`; `route_tree.rs`: redirect code generation |
| root rendering | `packages/router/src/components/router.rs`: `Router`, `RouterProps`; `components/outlet.rs`: `Outlet`; `contexts/outlet.rs`: `OutletContext::render` |
| links | `packages/router/src/components/link.rs`: `LinkProps`, `Link` |
| targets | `packages/router/src/navigation.rs`: `NavigationTarget`, conversion and `FromStr` implementations |
| router state | `packages/router/src/contexts/router.rs`: `RouterContext::new`, `current`, `push`, `replace`, `change_route`, `render_error`, `GenericRouterContext` |
| navigator/hooks | `packages/router/src/contexts/navigator.rs`: `navigator`, `Navigator`; `hooks/use_route.rs`: `use_route`; `hooks/use_router.rs`: `router`, `try_router`; `hooks/use_navigator.rs`: `use_navigator` |
| history injection | `packages/router/src/components/history_provider.rs`: `HistoryProvider`; `packages/history/src/lib.rs`: `History`, context functions; `packages/history/src/memory.rs`: `MemoryHistory` |
| feature gates | `packages/router/Cargo.toml`: `html`, `streaming`, `wasm-split`; `packages/router/src/lib.rs`: HTML exports; `components/router.rs`: streaming commit; `router-macro/src/route.rs`: lazy route loading |

## Official 0.7 docs

- [Routing overview](https://dioxuslabs.com/learn/0.7/essentials/router/)
- [Defining routes](https://dioxuslabs.com/learn/0.7/essentials/router/routes/)
- [Navigation](https://dioxuslabs.com/learn/0.7/essentials/router/navigation/)
- [Layouts](https://dioxuslabs.com/learn/0.7/essentials/router/layouts/)

Docs explain intent. Pinned code decides exact behavior.

## Version mismatches and gaps

1. Provider count mismatch. Navigation docs say "two" defaults but list three, then show `RouterConfig` as history override. Pinned router exposes generic `HistoryProvider`; concrete web/liveview provider selection lives in renderer integrations, not router crate.
2. Matching-order wording mismatch. Derive rustdoc lists query routes before static routes. Official route docs and `ParseRouteTree` use path specificity, static then dynamic then catch-all, with query/hash parsed on candidates.
3. Trait-name mismatch. Official catch-all prose says `FromSegments`; pinned symbol is `FromRouteSegments`.
4. Navigation prop wording mismatch. Official prose calls `Link` input `target`; pinned `LinkProps` and examples use `to`.
5. Nested-route example typo. Official comment says `/blog/:name`, while declaration and field use `id`; effective route is `/blog/:id`.
6. Outlet rustdoc parent mismatch. `components/outlet.rs` says `Outlet` must descend from `Link`; implementation requires router and outlet contexts. Treat sentence as typo.
7. Auth and HTTP status gap. Router source has no first-class async auth guard or HTTP 404/redirect status API. Those belong to components and fullstack server boundaries.
8. Platform source gap. Pinned repository router/history core proves generic and memory history behavior. Web and LiveView histories sit in renderer packages and were not needed for core API claims. Inspect selected renderer source before asserting provider-specific capabilities.
9. Official pages are mutable 0.7 explanatory docs, not blobs pinned to Dioxus SHA. Any contradiction is a version mismatch; local source remains authority.
