# Ownership, lifetime, and storage

## Owner model

Signals are `Copy` handles into generational storage. Copying handle does not copy `T` and does not extend owner lifetime. `use_signal` initializes once through `use_hook`; value belongs to current component scope and is dropped with scope owner.

Source: `packages/hooks/src/use_signal.rs:39-40`, `84-95`; `packages/signals/src/signal.rs:13-28`; lifecycle proof: `packages/signals/tests/create.rs:76-115`.

Follow one-way lifetime:

- Create state at or above longest-lived consumer.
- Pass handles down tree.
- Do not save child-owned signal or memo in ancestor state.
- Use `Signal::new_in_scope` only when explicit owner is known and stable.
- Use app globals only for truly app-wide state.

After owner drops, infallible accessors panic; `try_read`, `try_write`, and peers report dropped storage. `origin_scope()` exposes owner scope. `manually_drop()` invalidates handle. Source: `packages/signals/src/signal.rs:54-58`, `202-253`; `packages/signals/src/read.rs:75-169`; `packages/signals/src/write.rs:263-292`.

## Direct allocation

`Signal::new` uses current scope owner. Repeated calls outside one-time hook initialization allocate new entries retained until owner drops. Put direct construction inside `use_hook`, context-provider initializer, or another once-only owner setup. `leak_with_caller` has no owner and requires manual drop; keep it out of normal app code.

Source: `packages/signals/src/signal.rs:30-58`, `142-225`.

## Unsync and sync storage

Default `Signal<T>` uses `UnsyncStorage`. It suits component thread and supports non-`Send` values. `SyncSignal<T>` aliases `Signal<T, SyncStorage>`. `use_signal_sync` requires `T: Send + Sync + 'static` and returns sync storage.

Use sync storage only when handle crosses OS or executor thread boundary. A `move` async block on same Dioxus runtime does not by itself require `SyncSignal`. For `std::thread::spawn`, `tokio::spawn`, or APIs requiring `Send`, use `use_signal_sync` and ensure all captured values meet bounds.

Source: `packages/signals/src/signal.rs:17-22`; `packages/hooks/src/use_signal.rs:43-80`; example: `packages/signals/examples/send.rs:7-22`.

Sync storage changes storage synchronization, not reactive ownership:

- Owner scope can still unmount while worker holds copied handle.
- Worker lifetime must stop before or with owner lifetime.
- Runtime callbacks still route updates to owning app.
- Holding a sync read or write guard across blocking work or await still causes contention and borrow failures.

Boxed sync handles require `ReadSignal<T, SyncStorage>` or `WriteSignal<T, SyncStorage>` and compatible boxed storage. Storage type is part of handle type, so do not erase thread boundary accidentally. Source: `packages/signals/src/boxed.rs:438-539`.

## Globals

`Global` is lazy per application runtime, resolved through current virtual DOM. It is not one process-wide initialized value despite static declaration. Explicit keys control identity. Globals are poor defaults for libraries because separate component instances cannot naturally own separate state.

Source: `packages/signals/src/global/mod.rs:25-29`, `114-180`; warnings: `packages/signals/src/signal.rs:60-86`, `89-121`.
