# Source authority

## Pin

- Repository: `https://github.com/DioxusLabs/dioxus.git`
- Local checkout: `/root/repo/dioxus`
- Exact commit: `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`
- Immutable source base: `https://github.com/DioxusLabs/dioxus/blob/fda3dc9c2b10ddf4417edcbb98caa9613ac92d26/`

## Conflict protocol

1. Confirm local HEAD with `git -C /root/repo/dioxus rev-parse HEAD`.
2. Find public API re-export, then trace to implementation symbol.
3. Read implementation, associated macro expansion when syntax is generated, and matching repository test.
4. Use official docs to explain intent. Never use latest docs to override pinned implementation.
5. If rustdoc and implementation disagree, state both and choose implementation for behavior.
6. If behavior depends on feature flags or target cfg, name exact cfg branch.
7. Leave unresolved claim out of guidance. Report path, symbol, and missing evidence as gap.

## Evidence quality

| Evidence | Use | Authority |
|---|---|---|
| Local implementation at pin | Runtime behavior, feature order, panic and cleanup semantics | Highest |
| Macro implementation at pin | Generated props, defaults, conversion behavior | Highest for generated API |
| Repository tests at pin | Proven use and expected transitions | Strong supporting evidence |
| Rustdoc in same source file | Intended API contract | Explanatory when consistent |
| Official Dioxus 0.7 docs | Concepts and user-facing examples | Explanatory only |
| Latest docs.rs or website | Discovery | Version may differ, never decisive |

## Known source-doc conflict

`packages/dioxus/src/launch.rs`, `launch` rustdoc lines 27-39, lists renderer priority as liveview, server, native, desktop, mobile, web. `LaunchBuilder::new` lines 101-118 selects native, desktop, mobile, web, server, liveview. At this pin, constructor implementation decides actual selection. Prefer one renderer feature or explicit `LaunchBuilder::{web,desktop,mobile,server}` where available.

## Stable source links

- [`packages/dioxus/src/launch.rs`](https://github.com/DioxusLabs/dioxus/blob/fda3dc9c2b10ddf4417edcbb98caa9613ac92d26/packages/dioxus/src/launch.rs)
- [`packages/core/src/properties.rs`](https://github.com/DioxusLabs/dioxus/blob/fda3dc9c2b10ddf4417edcbb98caa9613ac92d26/packages/core/src/properties.rs)
- [`packages/core/src/global_context.rs`](https://github.com/DioxusLabs/dioxus/blob/fda3dc9c2b10ddf4417edcbb98caa9613ac92d26/packages/core/src/global_context.rs)
- [`packages/core/src/error_boundary.rs`](https://github.com/DioxusLabs/dioxus/blob/fda3dc9c2b10ddf4417edcbb98caa9613ac92d26/packages/core/src/error_boundary.rs)
- [`packages/core/src/virtual_dom.rs`](https://github.com/DioxusLabs/dioxus/blob/fda3dc9c2b10ddf4417edcbb98caa9613ac92d26/packages/core/src/virtual_dom.rs)
- [`packages/manganis/manganis-core/src/asset.rs`](https://github.com/DioxusLabs/dioxus/blob/fda3dc9c2b10ddf4417edcbb98caa9613ac92d26/packages/manganis/manganis-core/src/asset.rs)

Official docs: [Dioxus 0.7 essentials](https://dioxuslabs.com/learn/0.7/essentials/), [platform guides](https://dioxuslabs.com/learn/0.7/guides/), [docs.rs API](https://docs.rs/dioxus/0.7). These links explain APIs but float independently from local checkout.
