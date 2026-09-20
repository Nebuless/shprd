# Hydration, streaming, and SSR boundary

## Initial hydration workflow

1. Enable web `hydrate` feature for hydrated client. `Config::hydrate(true)` is available only under that feature, while compiling feature also makes `run` choose hydration through `cfg!(feature = "hydrate")`. Pins: `packages/web/Cargo.toml` feature `hydrate`, `packages/web/src/cfg.rs::Config::hydrate`, `packages/web/src/lib.rs::run`.
2. Ensure server emitted `window.initial_dioxus_hydration_data`, plus optional debug type and location arrays. Client decodes base64 bytes and builds `HydrationContext`. Pin: `packages/web/src/lib.rs::run`.
3. Client rebuilds VDOM in place without mutations because SSR DOM already exists, then `WebsysDom::rehydrate` binds VDOM IDs and listeners to existing nodes. Pins: `packages/web/src/lib.rs::run`, `packages/web/src/hydration/hydrate.rs::WebsysDom::rehydrate`.
4. Confirm initial server and client trees have same effective browser DOM shape. Markerless walk traverses rebuilt VDOM in document order and cursor reads actual DOM. Pins: `packages/web/src/hydration/walk.rs::WebsysDom::emit_scope`, `packages/web/src/hydration/cursor.rs::HydrationCursor`.

Completion: initial SSR content remains, handlers work, and first reactive update patches same nodes without mismatch.

## Markerless matching rules

- Components and fragments are transparent at current DOM level. Static and dynamic text contributions share text runs because browser merges adjacent text. Pin: `packages/web/src/hydration/walk.rs::WebsysDom::emit_dynamic_node_at_level`.
- Text split offsets use UTF-16 length to match browser `Text.length` and `splitText`. Pin: `packages/web/src/hydration/walk.rs::utf16_len`.
- Empty addressable text gets claimed if present or synthesized at cursor position. Pin: `packages/web/src/hydration/cursor.rs::HydrationCursor::empty_text_slot`.
- Cursor may step through parser-inserted, attribute-less wrapper elements. A real tag mismatch, missing node, detached root, or invalid text split returns `HydrationMismatch`. Pins: `HydrationCursor::map_element`, `HydrationCursor::over_roots`, `HydrationCursor::text_contrib`.
- Initial root scripts injected for hydration are filtered. If SSR emitted no roots, cursor hydrates inside mount parent. Pin: `packages/web/src/hydration/hydrate.rs::WebsysDom::start_hydration_at_scope`.

## Streaming hydration workflow

1. Server streaming emits resolved suspense containers and calls `window.dx_hydrate` with suspense path plus serialized hydration data. Pin: `packages/fullstack-server/src/streaming.rs::StreamingRenderer::replace_placeholder`.
2. Client registers callback through interpreter binding and queues `SuspenseMessage`. Pins: `packages/interpreter/src/lib.rs::minimal_bindings::register_rehydrate_chunk_for_streaming`, `packages/web/src/hydration/hydrate.rs::WebsysDom::rehydrate`.
3. Main web loop receives message and calls `rehydrate_streaming`. Pin: `packages/web/src/lib.rs::run`.
4. Client maps server suspense discovery path to client scope, resolves suspense with streamed nodes, flushes replacement, removes stream container, then hydrates resolved scope. Pin: `packages/web/src/hydration/hydrate.rs::WebsysDom::rehydrate_streaming_inner`.

Suspense paths depend on matching discovery order. Initial collection excludes retained primary branches until boundary resolves; empty chunks collect nested retained branches because no real DOM can drive walk. Pins: `packages/web/src/hydration/suspense.rs::WebsysDom::collect_initial_suspense`, `packages/web/src/hydration/suspense.rs::WebsysDom::collect_suspense_only`.

## SSR boundary

- `dioxus-web` consumes browser DOM. It doesn't render server HTML. SSR/fullstack server emits HTML and hydration payloads. Pins: `packages/web/src/lib.rs::run`, `packages/fullstack-server/src/ssr.rs::SsrRendererPool::render_to`.
- Web document uses `WebDocument` for client-only rendering. Hydrated fullstack client wraps it in `FullstackWebDocument` and supplies fullstack history context. Pins: `packages/web/src/document.rs::init_document`, `packages/web/src/document.rs::init_fullstack_document`.
- Router with `streaming` feature commits initial chunk after suspense resolves. Pin: `packages/router/src/components/router.rs::Router`.

## Edge matrix

Exercise changed area against cases present in `packages/playwright-tests/markerless-hydration-edges/src/main.rs`:

- Adjacent dynamic and static text, including emoji or non-BMP text.
- Empty text at start, middle, end, all-empty root, and streamed empty boundary.
- Components whose roots join parent text run.
- Conditional placeholders before, between, nested under, and after real siblings.
- `dangerous_inner_html`, whose children aren't represented in VDOM.
- SVG listener targets.
- Raw-text elements such as `textarea` and `pre`, including leading newline parser behavior.

Run browser assertions after hydration and after mutation. Initial matching alone misses broken node binding.

## Troubleshooting

| Symptom | Check | Source pin |
|---|---|---|
| Hydration panic says feature absent | `dioxus-web/hydrate` feature | `packages/web/src/lib.rs::run` |
| Immediate `HydrationMismatch` | Server/client tag order, parser wrappers, merged text, raw-text parsing | `packages/web/src/hydration/cursor.rs::HydrationCursor`, `packages/web/src/hydration/walk.rs::WebsysDom::emit_scope` |
| Emoji shifts later text updates | UTF-16 contribution lengths | `packages/web/src/hydration/walk.rs::utf16_len` |
| Empty content appears in wrong location after update | Empty text or virtual placeholder binding | `packages/web/src/hydration/cursor.rs::HydrationCursor::empty_text_slot` |
| Streamed boundary logs `ElementNotFound` | Resolved element `ds-<path>-r` absent or removed | `packages/web/src/hydration/hydrate.rs::WebsysDom::rehydrate_streaming_inner`, `packages/web/src/hydration/suspense.rs::path_to_resolved_suspense_id` |
| Streamed boundary maps wrong scope | Server/client suspense discovery order diverged | `packages/web/src/hydration/suspense.rs::SuspenseHydrationIds` |
| Streamed server error mismatches resolved subtree | Error payload path | Client removes resolved container, throws into boundary, and skips subtree hydration. `packages/web/src/hydration/hydrate.rs::WebsysDom::rehydrate_streaming_inner` |

## 0.7 explanatory note

[0.7 SSR](https://dioxuslabs.com/learn/0.7/essentials/fullstack/ssr) and [0.7 HTML streaming](https://dioxuslabs.com/learn/0.7/essentials/fullstack/streaming) explain why server HTML hydrates and suspense can stream. They don't define this SHA's markerless walker. Local 0.8-alpha source does.
