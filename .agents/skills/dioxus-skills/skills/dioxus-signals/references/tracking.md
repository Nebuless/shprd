# Dependency tracking and batching

## Subscription path

`SignalData<T>` holds value plus `Arc<Mutex<HashSet<ReactiveContext>>>`. A tracked read gets `ReactiveContext::current()` and subscribes it to that set. A write guard's drop takes subscriber set, calls `mark_dirty`, removes dead contexts, then restores live subscribers without holding lock during callbacks.

Source: `packages/signals/src/signal.rs:24-28`, `255-268`, `400-437`, `518-537`.

Treat every helper that reaches `read()` as tracked. Display formatting, call syntax, `cloned`, `with`, indexing, iteration, and collection helpers can subscribe a render, effect, memo, or resource. Use `peek` only when future changes must not rerun current reactive work.

## Dynamic dependency sets

Reactive contexts collect reads while callback runs. `reset_and_run_in` first removes old subscriptions, then runs callback, then installs dependencies read on that run. Conditional dependencies therefore change by branch:

```rust
let selected = use_memo(move || {
    if enabled() { count() } else { 0 }
});
```

When `enabled` is false, `count` is absent from memo's current dependency set. Test both branch transitions. Source: `packages/core/src/reactive_context.rs:133-209`.

## Notification and batching

There is no public `batch` function in `packages/signals` at pinned SHA. Batching comes from queued reactive callbacks and runtime scheduling:

- `ReactiveContext::new_with_origin` sends only when channel has no queued update, deduplicating pending callback work.
- Component contexts enqueue `SchedulerMsg::Immediate(scope)` when marked dirty.
- Memo invalidation marks dirty and queues a channel item; memo worker drains pending items before recomputing.
- Write notification starts when each write guard drops. Keep related writes in same synchronous app step so queued work can deduplicate.
- `.await` yields control and forms practical scheduling boundary. Do not promise atomic transactions across it.

Source: `packages/core/src/reactive_context.rs:45-67`, `102-130`; `packages/signals/src/memo.rs:49-82`.

Official docs call this "Automatic Batching" and say built-in hooks try to batch writes within current step, with await boundaries separating steps. Pinned code supports queued deduplication, but exposes no explicit transaction API or hard guarantee that arbitrary custom reactive contexts observe one callback for any set of writes.

## Feedback loops

A component that tracks a signal and writes it during same render can schedule itself repeatedly. Keep event writes outside render. When code must inspect current value without becoming dependent, use `peek` then normal `write`; avoid deprecated `write_silent`, which suppresses every subscriber and can leave distant UI stale.

Source: `packages/signals/src/signal.rs:276-397`.
