# Errors and testing

All claims use commit `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Error flow

`packages/core/src/lib.rs` defines `Element = Result<VNode, RenderError>`. A component can use `?` for errors convertible into captured render errors. `packages/core/src/global_context.rs`, `throw_error`, inserts an error into current scope so it bubbles to nearest boundary or root.

`packages/core/src/error_boundary.rs`, `ErrorBoundary`:

- Calls `use_error_boundary_provider` to provide `ErrorContext` to descendants.
- Reads first stored error.
- Renders `handle_error` when error exists.
- Renders children when no error exists.
- Uses nearest boundary because errors bubble through tree context.

`ErrorContext::insert_error` replaces stored error and marks subscribers dirty. `ErrorContext::clear_errors` removes it and marks subscribers dirty. Reset queues work; child success appears after following render cycle. Repository test `packages/core/tests/error_boundary.rs`, `clear_error_boundary`, needs multiple `render_immediate` calls before success output.

## Error choice

| Failure | Handle where | Reason |
|---|---|---|
| User input parse with inline correction | Component or event handler | Preserve form and precise message |
| Recoverable request state | Resource/result UI branch | Loading, retry, stale data need domain state |
| Unexpected subtree render failure | `ErrorBoundary` around subtree | Isolates failure and supplies fallback |
| Async/event error already in runtime scope | Return compatible `Result` or call `throw_error` | Sends error to nearest boundary |
| Fatal application setup failure | Launch boundary or process error | Component boundary may not exist yet |

Boundary only catches descendants. Handler identity participates in `ErrorBoundaryProps` equality through `Rc` pointer equality; recreating handler may affect memoization. Source: `ErrorBoundaryProps::eq`.

Panic capture differs on WASM. `CapturedPanic` rustdoc states WASM cannot catch unwinds, so test explicit error propagation rather than relying on panic boundary portability.

## Test level table

| Behavior | Harness | First action | Observable assertion |
|---|---|---|---|
| Pure RSX or prop rendering | `dioxus_ssr::render_element` | Build `Element` | HTML string or semantic fragment |
| Initial component tree | `VirtualDom::new` then `rebuild` or `rebuild_in_place` | Progress VDOM | SSR output or renderer mutations |
| Root with props | `VirtualDom::new_with_props` | Rebuild | Rendered value from props |
| Reactive rerender | VDOM plus event/state update | Process or render queued work | Changed output and focused mutation summary |
| Context ancestry | VDOM plus root/provider contexts | Render child | Nearest typed context value |
| Cleanup | VDOM with conditional child | Remove child and render | Drop-side observable state |
| Error fallback and reset | VDOM with `ErrorBoundary` | Throw, render fallback, clear, render again | Fallback then recovered child |
| Renderer-specific event or platform config | Target renderer or renderer oracle | Dispatch real event or launch target | User-visible state or config effect |

## VirtualDom facts

- `VirtualDom::new` and `new_with_props` do not progress rendering. Source: `packages/core/src/virtual_dom.rs`.
- `VirtualDom::prebuilt` rebuilds immediately.
- `with_root_context` inserts dependency before rendering.
- `mark_dirty` queues scope rerender if scope still exists.
- `wait_for_work` drains scheduler events and waits until work exists.
- Suspense tests must wait through `wait_for_suspense` when final resolved output matters.

Prefer output assertions for component contracts and mutation assertions for diff efficiency or event behavior. `packages/ssr/tests/simple.rs`, `simple` proves SSR pattern. `packages/core/tests/lifecycle.rs`, `manual_diffing` proves explicit dirty render pattern. `packages/core/tests/error_boundary.rs`, `clear_error_boundary` proves reset sequence.

## Minimum scenario matrix

| Changed area | Required cases |
|---|---|
| Props or children | Required/defaulted input and changed prop rerender |
| Context | Provider found, nearest provider wins, missing path if optional API |
| Hook order | Initial render and rerender with condition changed |
| Effect or cleanup | Mount, rerun trigger if reactive, unmount |
| Error boundary | Child success, child error fallback, clear and retry |
| Asset | Unbundled compile plus bundled URL through matching `dx` |
| Launch | Intended feature target plus accidental multi-feature guard |

## Test traps

- First render alone cannot prove hook retention.
- `NoOpMutations` proves render completion, not output shape.
- SSR cannot prove client effects because effects do not run on server.
- One `render_immediate` may only advance one queued boundary transition.
- Panic-catching expectations are not portable to WASM.
- Plain Cargo build cannot prove CLI-patched asset path.

Official docs: [error handling](https://dioxuslabs.com/learn/0.7/essentials/basics/error_handling/), [testing guide](https://dioxuslabs.com/learn/0.7/guides/testing/). Source and repository tests above decide exact transitions.
