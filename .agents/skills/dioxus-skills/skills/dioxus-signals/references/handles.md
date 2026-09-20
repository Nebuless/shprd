# Signal handles

## Choose by capability

| Type | Capability | Storage | Use |
| --- | --- | --- | --- |
| `Signal<T>` | Read and write concrete signal | `UnsyncStorage` by default | Component-owned mutable state |
| `SyncSignal<T>` | `Signal<T, SyncStorage>` | Thread-safe | Handle and value must cross threads |
| `ReadSignal<T, S>` | Boxed `Readable` | Generic boxed storage | Read-only props or heterogeneous readable inputs |
| `WriteSignal<T, S>` | Boxed `Writable`, also readable | Generic boxed storage | Mutable props or heterogeneous writable inputs |
| `Memo<T>` | Derived readable, also implements `Writable` at this SHA | `UnsyncStorage` | Synchronous cached derivation |
| `Resource<T>` | Reactive `Option<T>` plus task state | `UnsyncStorage` | Async derivation |

`ReadSignal` and `WriteSignal` are capability-erased wrappers, not restricted aliases of one `Signal`. `ReadSignal::new_maybe_sync` stores a boxed `Readable`; `WriteSignal::new_maybe_sync` stores a boxed `Writable`. Both are `Copy` handles. See `packages/signals/src/boxed.rs:18-76` and `218-243`.

Prefer concrete trait bounds in ordinary Rust helpers:

```rust
fn label(value: &impl Readable<Target = String>) -> String {
    value.cloned()
}
```

Prefer `ReadSignal<T>` or `WriteSignal<T>` at component prop boundaries when RSX conversion and one stable concrete prop type matter. `Signal`, `Memo`, mapped signals, globals, and `WriteSignal` have explicit conversions into `ReadSignal` in `packages/signals/src/boxed.rs:153-215`.

## Read paths

- `read()` returns a guard and subscribes current reactive context.
- `signal()` and `cloned()` clone the value through a tracked read.
- `with()` runs a closure under a tracked read guard.
- `peek()` and `with_peek()` read without subscription.
- `try_read()` and `try_peek()` preserve dropped or conflicting borrow errors instead of panicking.
- `read_unchecked()` means the returned guard has a widened static lifetime. Runtime borrow checks remain active. It is not an unchecked memory access.

Source: `packages/signals/src/read.rs:41-230`, `packages/signals/src/signal.rs:400-437`.

## Write paths

- `write()` returns `WritableRef`, a `WriteLock` that dereferences to `&mut T`.
- `set`, `with_mut`, `replace`, `take`, collection helpers, and operator assignments acquire a write guard internally.
- Subscriber invalidation occurs when signal write metadata drops, not when `write()` is called. Keeping a guard alive delays notification.
- Match an enum through `write.deref_mut()` or `match &mut *write`, not against `WriteLock` itself.
- `try_write()` exposes borrow failure. `write_unchecked()` widens handle lifetime but still performs runtime checks.

Source: `packages/signals/src/write.rs:40-261`, `263-410`; `packages/signals/src/signal.rs:440-460`, `518-537`.

## Mapped views

`map` and `map_mut` project references without cloning or allocating a new value. Their subscribers still follow whole source signal. Any source write can rerun a reader even when projected field is unchanged. Use `Memo` when equality suppression is enough. Use Dioxus stores for fine-grained field subscriptions.

Source: `packages/signals/src/read.rs:171-204`; `packages/signals/src/write.rs:294-332`.
