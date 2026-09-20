---
name: dioxus-skills
description: Routes Dioxus framework work to focused, source-grounded skills. Use when implementing, reviewing, or maintaining Dioxus applications and libraries.
metadata:
  invocation: model
---

# Dioxus skills

Use this model-invoked router when Dioxus work spans package boundaries. Load only matching leaf:

- Read [`skills/dioxus-core/SKILL.md`](skills/dioxus-core/SKILL.md) for components, props, context, hooks, lifecycle, errors, launch, and component tests.
- Read [`skills/dioxus-web/SKILL.md`](skills/dioxus-web/SKILL.md) for WASM, browser rendering, hydration, web assets, and deployment.
- Read [`skills/dioxus-desktop/SKILL.md`](skills/dioxus-desktop/SKILL.md) for desktop windows, event loops, menus, IPC, native integration, and packaging.
- Read [`skills/dioxus-mobile/SKILL.md`](skills/dioxus-mobile/SKILL.md) for Android or iOS lifecycle, permissions, native plugins, and device builds.
- Read [`skills/dioxus-fullstack/SKILL.md`](skills/dioxus-fullstack/SKILL.md) for server functions, Axum, SSR, streaming, payloads, and server security.
- Read [`skills/dioxus-routing/SKILL.md`](skills/dioxus-routing/SKILL.md) for typed routes, layouts, navigation, history, redirects, and route tests.
- Read [`skills/dioxus-signals/SKILL.md`](skills/dioxus-signals/SKILL.md) for reactive state, dependency tracking, ownership, borrowing, and async state.
- Read [`skills/dioxus-platform-interop/SKILL.md`](skills/dioxus-platform-interop/SKILL.md) for JavaScript, DOM, Web Components, native FFI, React migration, and CSS boundaries.
- Consult [`generated/skills.json`](generated/skills.json) for the generated metadata catalog.

## Package policy

- Keep root routing-only. Put workflows in `skills/<name>/SKILL.md` and branch-only detail in that leaf's `references/`.
- Make leaves model-invoked: retain trigger-oriented `description`, omit `disable-model-invocation`, and set `metadata.invocation: model`.
- Resolve every relative pointer from containing skill root. Keep references one hop from `SKILL.md`.
- Run `mise run generate-index` after metadata changes; parsed `SKILL.md` metadata remains the source of truth.
- Run `mise run validate-skills`; finish only when schema and writing-style checks both pass.
