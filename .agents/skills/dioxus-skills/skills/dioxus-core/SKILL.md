---
name: dioxus-core
description: Builds and repairs Dioxus application foundations. Use when working on cross-platform launch, component boundaries, props or children, context, hooks and cleanup, error boundaries, component tests, assets, or conflicts between Dioxus docs and source.
metadata:
  invocation: model
---

# Dioxus core

Use local source as contract. This skill is pinned to Dioxus commit `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26` in `/root/repo/dioxus`.

## Workflow

1. Run `git -C /root/repo/dioxus rev-parse HEAD`. Stop using this skill as behavioral authority if output differs from pinned commit. Inspect changed symbols before editing application code.
2. Classify task with table below. Read every listed reference before choosing API.
3. Trace application call site to pinned symbol. Treat re-exports as navigation, not implementation evidence.
4. Make smallest change preserving hook order, component identity, context scope, and renderer feature selection.
5. Test observable behavior through `VirtualDom`, SSR, or target renderer. Include rerender or failure path when task touches retained state, context, lifecycle, or errors.
6. Run validation checklist. Finish only when every applicable item has source evidence and runtime evidence.

## Reference routing

| Task | Read | Decision required |
|---|---|---|
| Select renderer, inject root context, pass platform config | [`references/launch-and-assets.md`](references/launch-and-assets.md) | `launch` or explicit `LaunchBuilder`; one renderer feature or intentional override |
| Bundle image, stylesheet, font, or generated file | [`references/launch-and-assets.md`](references/launch-and-assets.md) | required `asset!` or optional `option_asset!`; source path or bundled URL |
| Split component, define props, accept children | [`references/components.md`](references/components.md) | local props or context; `#[component]` or explicit `Props`; required or defaulted children |
| Share state through tree or read context outside render | [`references/context-and-lifecycle.md`](references/context-and-lifecycle.md) | cached hook lookup or live runtime lookup; nearest provider or root provider |
| Add hook, effect, task, cleanup, or diagnose retained state | [`references/context-and-lifecycle.md`](references/context-and-lifecycle.md) | reactive hook or retained nonreactive value; scope-bound or root-bound task |
| Propagate render or event error, add fallback, reset failure | [`references/errors-and-testing.md`](references/errors-and-testing.md) | local handling or nearest `ErrorBoundary`; clear and retry semantics |
| Test initial render, rerender, lifecycle, context, or fallback | [`references/errors-and-testing.md`](references/errors-and-testing.md) | SSR output, mutation behavior, or renderer-level event behavior |
| Docs disagree with code or examples differ by version | [`references/source-authority.md`](references/source-authority.md) | source symbol at pinned commit wins; record unresolved ambiguity |

## Architecture choices

| Need | Prefer | Avoid | Source anchor |
|---|---|---|---|
| Leaf-owned input | Typed props | Context for one parent-child edge | `packages/core/src/properties.rs`, `Properties` |
| Arbitrary nested content | `children: Element` | Callback returning RSX unless deferred rendering is required | `packages/core-macro/src/props/mod.rs`, `FieldInfo::new` |
| Shared descendant dependency | `use_context_provider` plus `use_context` | Prop drilling through unrelated components | `packages/hooks/src/use_context.rs` |
| Read context in event or task | `consume_context` or `try_consume_context` while runtime is active | Calling a context hook in callback | `packages/core/src/global_context.rs` |
| Reactive retained state | `use_signal` or matching reactive hook | `use_hook` with `Rc<RefCell<_>>` expecting UI updates | `packages/core/src/global_context.rs`, `use_hook` |
| Scope cleanup | `use_drop` or value with `Drop` retained by hook | Assuming effects run on server | `packages/core/src/global_context.rs`, `use_drop` |
| Expected local failure | Handle `Result` near operation | Replacing useful local recovery with broad boundary | `packages/core/src/error_boundary.rs`, `ErrorBoundary` |
| Subtree render failure | `ErrorBoundary` | Assuming it catches failures above itself | `packages/core/src/error_boundary.rs`, `ErrorBoundary` |
| Pure rendered output | `dioxus_ssr::render_element` | Full renderer launch | `packages/ssr/src/renderer.rs`, `render_element` |
| Stateful rerender | `VirtualDom` plus rebuild/render calls | Single first-render assertion | `packages/core/src/virtual_dom.rs`, `VirtualDom` |

## Hard edges

- Hook slots are positional. Same-type hook reordering can return wrong retained state without panic. Different-type mismatch can panic outside debug hot-patch recovery.
- `use_context` and `try_use_context` cache first lookup in hook storage. Runtime `consume_context` and `try_consume_context` read current tree each call.
- `use_drop` runs when component drops, including SSR. Gate platform-specific cleanup.
- `ErrorBoundary` catches errors from descendants, switches to handler output, and needs `ErrorContext::clear_errors` before children can render again.
- `Asset` formats to a resolved source path outside bundled mode and `/assets/...` under bundled mode. It is not a stable file-system path inside a bundle.
- `LaunchBuilder::new` implementation order is `native`, `desktop`, `mobile`, `web`, `server`, `liveview`. Its rustdoc lists another priority. Follow implementation at pinned commit or select explicit constructor.

## Validation checklist

- [ ] Local Dioxus HEAD equals `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.
- [ ] Every behavioral claim used in solution names local path and symbol from references.
- [ ] Enabled renderer features match intended target; multiple features are intentional.
- [ ] Root component has `fn() -> Element` when passed to public launch API.
- [ ] Props remain `Clone + 'static`; memoization behavior is understood for explicit props.
- [ ] Children requiredness matches API contract, including implicit empty default.
- [ ] Hook calls stay unconditional and in same order on every render.
- [ ] Context provider scope and cached versus live lookup match update behavior.
- [ ] Scope-bound tasks and cleanup are exercised through unmount or rerender where applicable.
- [ ] Error test covers fallback and reset or retry path where applicable.
- [ ] Asset test uses `dx` bundling when resolved bundled URL matters.
- [ ] Test asserts observable output or mutations after initial render and relevant rerender.
- [ ] Official docs links explain usage only; local pinned source decides conflicts.
