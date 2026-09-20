# Testing and performance

## Behavioral tests

Use `VirtualDom` when claim concerns components:

1. Render once with `rebuild_in_place` or mutation capture.
2. Count parent, child, memo, or effect runs in external test state.
3. Mutate through event-equivalent closure or scope execution.
4. Drive `render_immediate` until scheduled work settles.
5. Assert observable render or run counts, not subscriber internals.

Pinned examples: `packages/signals/tests/subscribe.rs:12-95` proves only readers rerun; `packages/signals/tests/memo.rs:11-147` proves memo recomputation and equality suppression; `packages/signals/tests/create.rs:76-115` proves scope cleanup.

For dependency tracking, test all transitions: inactive dependency write does nothing, controlling dependency switches branch, newly active dependency triggers, then switching back unsubscribes it. `ReactiveContext::reset_and_run_in` behavior is shown at `packages/core/src/reactive_context.rs:152-194`.

For borrow behavior, prefer `try_read` and `try_write` when failure is expected. Assert error rather than catching panic. Keep one test for async guard misuse only when project has deterministic executor control; never add sleeps to provoke overlap.

For resource behavior, use controllable futures or channels. Prove dependency change cancels old task, new task result wins, and owner drop stops pending work. Test `Pending`, `Ready`, `Stopped`, and `Paused` only when feature uses them.

## Performance model

- Signal handle copy is cheap; inner access adds generational lookup, runtime borrow check, and storage synchronization.
- Tracked read also touches current reactive context and subscriber set.
- Call syntax clones `T`; use short `read` or `with` for large values.
- `peek` saves subscription work only when untracked semantics are correct.
- Every write guard drop notifies subscribers even if assigned value equals old value. Plain `Signal` has no equality suppression.
- `Memo` pays computation plus `PartialEq`, then suppresses downstream notification when output is unchanged.
- `map` avoids clone but retains whole-source invalidation. Stores provide finer granularity.
- `SyncStorage` pays synchronization. Keep `UnsyncStorage` on local UI path.
- Long-lived guards increase contention and delay write notifications.

Do not claim "zero cost" literally. Official docs use phrase for no rerender when value is unobserved, while same page notes generational indirection and lock overhead. Measure expensive derivations and rerender counts in app workload before adding memo.

## Review checklist

- Public boundary grants minimum capability.
- Tracked reads appear only where rerun is wanted.
- Writes happen outside render and guards end promptly.
- Memo output equality is cheaper than avoided work.
- Async code owns values across await.
- Owner outlives callbacks, tasks, and threads.
- Sync storage appears only at real cross-thread boundary.
- Tests cover rerender, branch dependency, immediate memo freshness, and relevant lifetime path.
