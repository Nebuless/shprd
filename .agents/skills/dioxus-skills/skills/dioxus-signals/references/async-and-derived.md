# Memos, resources, and async edges

## Memo contract

`Memo::new` runs derivation immediately inside a fresh reactive context. Dependency changes mark memo dirty and queue recomputation. Recompute compares `T: PartialEq`; downstream memo subscribers update only when value differs.

A dirty memo read is synchronous. `Memo::try_read_unchecked` drops inner read, recomputes before returning, then subscribes current context to memo output. Reading memo immediately after dependency write therefore returns fresh value even before queued worker runs.

Source: `packages/signals/src/memo.rs:31-85`, `120-139`, `158-215`. Regression proof: `packages/signals/tests/memo.rs:149-178`.

`GlobalMemo<T>` is `Global<Memo<T>, T>`. It resolves lazily once per current application runtime. Its constructor is a function pointer, and library globals reduce support for multiple independent component instances. Source: `packages/signals/src/global/memo.rs:6-27`; `packages/signals/src/global/mod.rs:25-29`, `174-205`.

Although `Memo<T>` implements `Writable` at this SHA, treat memo as derived state in application design. Writing cached output can temporarily break equivalence with derivation until a dependency invalidates it.

## Resource contract

`use_resource` owns value `Signal<Option<T>>`, task, state, callback, waker, and a reactive context. It:

1. Sets state `Pending`.
2. Creates future under `reset_and_run_in`, capturing reads during future construction.
3. Polls every future poll under same reactive context, capturing reads that happen after awaits.
4. On dependency notification, cancels old task and starts new one.
5. On completion, sets `Ready`, stores `Some(result)`, and wakes resource awaiters.

Source: `packages/hooks/src/use_resource.rs:24-97`.

Use `value()` and `state()` for read-only handles. `pending()` and `finished()` peek and therefore do not subscribe despite `pending` doc not spelling that out. `clear()` resets value without changing running task. `cancel()` sets `Stopped`; `restart()` cancels then starts new task but callback itself sets `Pending`. Source: `packages/hooks/src/use_resource.rs:166-445`.

## Async safety

Never carry `ReadableRef` or `WritableRef` across `.await`. Runtime borrow stays active while future yields, blocking renders, event handlers, memo recomputation, or sibling tasks that need conflicting access.

```rust
let input = state.cloned();
let output = transform(input).await;
state.set(output);
```

For read-modify-write around async work, choose conflict semantics explicitly:

- Last completion wins: clone input, await, set output.
- Latest request wins: use `use_resource` dependency restart and cancellation.
- Merge with current state: await owned result, then reopen short `with_mut` closure.
- Preserve every event: serialize through task or channel rather than holding write guard.

Cancellation drops old future. Side effects already performed outside future cannot be rolled back. Resource result type should carry external errors, usually `Resource<Result<T, E>>`.

Source guidance: `packages/signals/docs/signals.md:85-135`; `packages/signals/docs/memo.md:56-116`; implementation: `packages/hooks/src/use_resource.rs:47-87`.
