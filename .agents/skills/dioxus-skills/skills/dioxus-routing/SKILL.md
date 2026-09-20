---
name: dioxus-routing
description: Builds and diagnoses Dioxus 0.7 routing. Use when working with Routable enums, route parameters, nested layouts, Router, Link, Outlet, navigation history, redirects, guards, not-found routes, SSR, fullstack, or router tests.
metadata:
  invocation: model
---

# Dioxus routing

1. Verify `/root/repo/dioxus` is at `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`. If SHA differs, treat this skill as version-mismatched and inspect current source before applying it.
2. Model URLs as a `Routable` enum. Prefer typed variants and typed `NavigationTarget`s over string paths. Read [`references/routes.md`](references/routes.md) for derive syntax, params, query, hashes, redirects, not-found routes, and matching edge cases.
3. Compose rendering with `Router`, layouts, and one `Outlet` per active layout level. Read [`references/rendering-and-navigation.md`](references/rendering-and-navigation.md) for nesting, links, hooks, history, and platform behavior.
4. Put access policy at the correct boundary. Use route rendering for local UI gates, `RouterConfig::on_update` for synchronous route replacement, and server middleware or server functions for authorization. Read [`references/fullstack-and-security.md`](references/fullstack-and-security.md) for SSR, hydration, fullstack, redirects, and auth limits.
5. Prove URL round trips and rendered outcomes. Read [`references/testing.md`](references/testing.md) for executable tests, failure cases, and source-backed checks. Finish only when parse, display, navigation, and render behavior used by the app are covered.

Pinned implementation wins over explanatory docs. [`references/source-map.md`](references/source-map.md) maps every claim to exact source symbols and labels known 0.7 documentation mismatches.
