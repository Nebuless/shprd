---
name: dioxus-signals
description: Builds and diagnoses Dioxus 0.7 reactive state. Use when working with Signal, ReadSignal, WriteSignal, Memo, GlobalMemo, Resource, dependency tracking, borrow failures, ownership, batching, async state, or cross-thread SyncSignal work.
metadata:
  invocation: model
---

# Dioxus signals

Use local Dioxus source at `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26` as authority. If `/root/repo/dioxus` differs, inspect changed source before applying this skill.

1. Classify state boundary. Use `Signal<T>` for owned mutable state, `ReadSignal<T>` for readable interface erasure, `WriteSignal<T>` for writable interface erasure, `Memo<T>` for synchronous derived state, and `Resource<T>` for reactive async work. Read [`references/handles.md`](references/handles.md) before choosing public parameter types.
2. Mark every read as tracked or untracked. `read`, call syntax, formatting, and readable helpers subscribe current `ReactiveContext`; `peek` does not. Read [`references/tracking.md`](references/tracking.md) when rerenders, effects, memos, or batching behave unexpectedly.
3. Keep runtime guards short. Finish every `ReadableRef` or `WritableRef` before conflicting access or any `.await`. Compute owned data first, await, then reopen a write guard. Read [`references/async-and-derived.md`](references/async-and-derived.md) for memo freshness, resource restarts, cancellation, and async races.
4. Place owner deliberately. Prefer `use_signal` or `use_memo`; create direct signals only in one-time initialization. Keep handles below their owner, or choose an explicit ancestor scope or app global. Read [`references/ownership-and-storage.md`](references/ownership-and-storage.md) before hoisting, global state, manual allocation, or thread crossing.
5. Prove observable behavior. Count component or computation runs, test conditional dependencies across both branches, exercise an immediate memo read after a dependency write, and test unmount or cancellation when lifetime matters. Use [`references/testing-and-performance.md`](references/testing-and-performance.md) for source-backed checks and cost choices.

Finish when every read has intended subscription semantics, no guard crosses an async yield, owner outlives all handles, storage matches thread boundary, and tests observe updates through a real `VirtualDom` or reactive context.

Official [Reactive Signals](https://dioxuslabs.com/learn/0.7/essentials/basics/signals) explains user-facing behavior. [`references/source-map.md`](references/source-map.md) records exact pinned implementation paths, examples, tests, and documentation gaps. Local source wins on conflict.
