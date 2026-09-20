# Pinned source map

## Authority

- Repository: `https://github.com/DioxusLabs/dioxus.git`
- Local checkout: `/root/repo/dioxus`
- Exact SHA: `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`
- Supporting docs: [Dioxus 0.7 Reactive Signals](https://dioxuslabs.com/learn/0.7/essentials/basics/signals)

## Implementation

| Topic | Exact path and locator |
| --- | --- |
| `Signal`, `SyncSignal`, owner constructors | `packages/signals/src/signal.rs:13-28`, `30-225` |
| Tracking, peek, write notification | `packages/signals/src/signal.rs:255-268`, `400-460`, `518-537` |
| `Readable`, guards, helpers | `packages/signals/src/read.rs:11-298` |
| `Writable`, `WriteLock`, helpers | `packages/signals/src/write.rs:10-410` |
| `ReadSignal`, `WriteSignal`, conversions | `packages/signals/src/boxed.rs:18-243`, `305-539` |
| `Memo` invalidation and synchronous freshness | `packages/signals/src/memo.rs:17-228` |
| `GlobalMemo` | `packages/signals/src/global/memo.rs:6-27` |
| Global lazy resolution | `packages/signals/src/global/mod.rs:25-29`, `94-205` |
| Reactive context dependency reset and scheduling | `packages/core/src/reactive_context.rs:45-67`, `97-250` |
| `use_signal`, `use_signal_sync` | `packages/hooks/src/use_signal.rs:4-95` |
| `use_resource`, task control, state/value handles | `packages/hooks/src/use_resource.rs:24-97`, `99-560` |
| Signal lifecycle and async warnings | `packages/signals/docs/signals.md:1-165` |
| Memo lifecycle and async warnings | `packages/signals/docs/memo.md:1-147` |

## Examples and tests

| Behavior | Exact path |
| --- | --- |
| Read-only prop conversion | `examples/04-managing-state/read_signal.rs:1-37` |
| Cross-thread signal | `packages/signals/examples/send.rs:1-23` |
| Component subscriptions | `packages/signals/tests/subscribe.rs:12-95` |
| Memo rerun and immediate freshness | `packages/signals/tests/memo.rs:11-178` |
| Scope-owned drop | `packages/signals/tests/create.rs:76-115` |

## Documentation mismatches and gaps

- Official docs say `WriteSignal` is equivalent to `Signal`. Pinned source defines boxed writable type erasure in `packages/signals/src/boxed.rs:218-243`; it is not a type alias and can wrap any compatible `Writable`.
- Official docs describe automatic batching at app-step level. Pinned source has queue deduplication in reactive contexts and memo worker, but no public signal `batch` API and no general transaction primitive.
- Official docs example says `let cur = state.read().clone()` releases guard immediately, then shows `*state.write() = *cur + 1`; cloned `i32` is a value, so dereferencing `cur` is inconsistent. Use `state.cloned()` or `let cur = *state.read()` and then `state.set(cur + 1)`.
- Official docs say `use_signal` registers `signal.dispose()` on component drop. Pinned implementation expresses cleanup through scope owner and generational storage; public method in this source is `manually_drop`, not `dispose`.
- Official docs present await boundaries as batching barriers. This is useful runtime guidance, but exact scheduler paint ordering is outside `packages/signals` and should not be treated as atomicity guarantee.
- Pinned signals package has no dedicated benchmark suite. Performance guidance rests on implementation shape and observable rerender tests, not benchmark numbers.
