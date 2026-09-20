# Context and lifecycle

All claims use commit `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Context choice

| Call site | API | Lookup behavior | Missing value |
|---|---|---|---|
| Component render, value stable for component lifetime | `use_context<T>()` | First lookup cached in hook slot | Panics |
| Component render, optional value stable for component lifetime | `try_use_context<T>()` | First lookup cached in hook slot | `None` |
| Component provider | `use_context_provider(|| value)` | Provides once through hook initialization | Returns provided clone |
| Event, spawned task, or other active-runtime code | `consume_context<T>()` | Reads current scope ancestry each call | Panics |
| Same, optional | `try_consume_context<T>()` | Reads current scope ancestry each call | `None` |
| VirtualDom setup | `VirtualDom::with_root_context` | Inserts into base scope before rendering | N/A |
| Launch setup | `LaunchBuilder::with_context` | Renderer injects root context from factory | N/A |

Sources: `packages/hooks/src/use_context.rs`, all three hooks; `packages/core/src/global_context.rs`, consume and provide functions; `packages/core/src/virtual_dom.rs`, root context methods.

Context search starts at current scope, then walks parents. Provider storage holds one value per concrete `TypeId`; providing same type in same scope replaces previous value. Source: `packages/core/src/scope_context.rs`, `Scope::consume_context`, `Scope::provide_context`.

Cached hook lookup does not follow a newly inserted nearer provider after first render. If provider topology can change while consumer scope survives, remount consumer or use live runtime lookup at appropriate active-runtime call site.

## Hook storage

`packages/core/src/scope_context.rs`, `Scope::use_hook`:

1. Reads current positional `hook_index`.
2. Increments index.
3. Clones retained value when slot has requested type.
4. Runs initializer when no matching value exists.
5. Pushes new slot, replaces mismatched slot only during debug hot patch, otherwise panics.

Same-type hook reorder is most dangerous because downcast succeeds and wrong state can move silently. Keep every hook unconditional, top-level in component or custom hook, and in stable order. Initializer closures and event handlers are not hook call sites.

`use_hook` retains and clones but does not subscribe UI to changes. Use reactive hooks such as `use_signal` when writes must schedule rendering. Source: `packages/core/src/global_context.rs`, `use_hook`.

## Lifecycle decision table

| Need | API | Timing and ownership | Source |
|---|---|---|---|
| Run after rendered changes | `use_effect` | Queued after render; tracks reactive reads; deduplicates queue | `packages/hooks/src/use_effect.rs`, `use_effect` |
| Run future owned by component | `spawn` | Task canceled when component drops | `packages/core/src/global_context.rs`, `spawn` |
| Run task beyond component | `spawn_forever` | Runs in root scope; context calls see root | same file, `spawn_forever` |
| Cleanup on component drop | `use_drop` | Retained lifecycle value calls closure on drop, including SSR | same file, `use_drop` |
| Retain value with paired cleanup | `use_hook_with_cleanup` | Clones retained value into one drop closure | same file, `use_hook_with_cleanup` |
| Before each render | `use_before_render` | Registers callback through hook | same file, `use_before_render` |
| After each render | `use_after_render` | Registers callback through hook | same file, `use_after_render` |

## Edges

- Effects do not run on server, while `use_drop` does. Cleanup touching browser APIs needs target gating.
- `spawn_forever` changes context scope and lifetime. Use only for work that truly outlives component.
- Reactive reads inside `use_effect` trigger future runs. Reads outside callback do not establish same effect dependency.
- `use_drop` closure is captured on first hook initialization. Values copied into it are initial-render captures unless handle itself observes current state.
- Hook initializer executes again after component remount because new scope owns new hook list.

## Verification pattern

1. Build `VirtualDom` and perform initial rebuild.
2. Change condition or reactive input.
3. Render queued work.
4. Assert retained state stayed with logical hook.
5. Remove component and assert scope task cancellation or cleanup side effect.
6. For context, test nearest-provider shadowing and missing-provider branch separately.

Repository examples: `packages/core/tests/context_api.rs`, `state_shares`; `packages/core/tests/lifecycle.rs`, `manual_diffing`; `packages/core/tests/use_drop.rs`.

Official docs: [state and hooks](https://dioxuslabs.com/learn/0.7/essentials/state/), [context](https://dioxuslabs.com/learn/0.7/essentials/basics/context/), [side effects](https://dioxuslabs.com/learn/0.7/essentials/state/effects/). Source above decides lifecycle behavior.
