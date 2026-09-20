# Launch and renderer

## Source baseline

- Repository: `https://github.com/DioxusLabs/dioxus.git`
- SHA: `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`
- Local checkout: `/root/repo/dioxus`

## Choose launch path

1. For normal app code, call `dioxus::launch(app)` or `LaunchBuilder`. Platform selection reaches `dioxus_web::launch::launch` only when selected platform is Web. Fullstack server builds take server launch path instead. Pin: `packages/dioxus/src/launch.rs::LaunchBuilder::launch`.
2. For renderer-level control, use `dioxus_web::launch_cfg(root, Config)` or build `VirtualDom` and use `launch_virtual_dom`. The former wraps `Config` into generic platform config; the latter starts `run` with `wasm_bindgen_futures::spawn_local`. Pins: `packages/web/src/launch.rs::launch_cfg`, `packages/web/src/launch.rs::launch_virtual_dom`.
3. Use `Config::rootname` for element ID in current document, `rootelement` for a concrete `web_sys::Element`, or `rootnode` for any `web_sys::Node`. Default ID is `main`. Pins: `packages/web/src/cfg.rs::Config`, `packages/web/src/cfg.rs::Config::default`.
4. Use `Config::history` only to replace history context. Renderer inserts configured history before document initialization. Pins: `packages/web/src/cfg.rs::Config::history`, `packages/web/src/lib.rs::run`.

Completion: selected launch branch and mount root match actual Cargo feature set and generated page.

## Understand render loop

`dioxus_web::run` performs these phases:

1. Installs configured history and document contexts when `document` feature is enabled.
2. Creates `WebsysDom` and chooses rebuild or hydration.
3. Client-only path calls `VirtualDom::rebuild`, then flushes interpreter edits.
4. Main loop waits for VDOM, hydration, or devtools work; renders immediately; flushes edits.

Pin: `packages/web/src/lib.rs::run`.

`WebsysDom::new` resolves root, initializes JS interpreter, installs delegated event callback, and registers web event converter. Missing named root logs browser error and creates a detached body element rather than mounting to existing document body. Treat missing `#main` as broken template, not valid fallback. Pin: `packages/web/src/dom.rs::WebsysDom::new`.

## Trace mutations

`WebsysDom` implements `WriteMutations` by forwarding traversal, creation, insertion, replacement, attributes, text, listener, and removal operations to interpreter. Pin: `packages/web/src/mutations.rs::impl WriteMutations for WebsysDom`.

- `AttributeValue::Text`, `Float`, `Int`, and `Bool` become text values. `None` removes attribute. Other variants are unreachable at this boundary. Pin: `WriteMutations::set_attribute` in `packages/web/src/mutations.rs`.
- Event listener bubbling comes from `dioxus_core_types::event_bubbles`. `mounted` is queued separately and dispatched after interpreter flush, when nodes exist. Pins: `WriteMutations::add_event_listener`, `WebsysDom::flush_edits`.
- Browser callback walks from event target to ancestor carrying `data-dioxus-id`, rejects mismatched JS event classes, dispatches through runtime, then calls browser `prevent_default` when Dioxus event disables default action. Pins: `packages/web/src/dom.rs::walk_event_for_id`, `packages/web/src/dom.rs::WebsysDom::new`.

## Browser event and mounted checks

For event bugs:

1. Confirm target or ancestor has valid `data-dioxus-id`.
2. Confirm browser event class matches named event. A plain `Event` named `keydown` is dropped before unchecked keyboard conversion.
3. Confirm handler changed Dioxus event default action before expecting browser `preventDefault`.
4. For `mounted`, inspect after mutation flush, not during node creation.

Pins: `packages/web/src/dom.rs::walk_element_for_id`, `packages/web/src/dom.rs::WebsysDom::new`, `packages/web/src/mutations.rs::WebsysDom::flush_queued_mounted_events`.

## Common failures

| Symptom | Inspect | Pinned cause |
|---|---|---|
| Blank page, console says root missing | Custom `index.html`, configured root | Default mount ID is `main`; missing ID creates detached body. `packages/web/src/cfg.rs::Config::default`, `packages/web/src/dom.rs::WebsysDom::new` |
| Handler never fires | Event target ancestry and JS event class | Dispatch requires valid Dioxus ID and matching event type. `packages/web/src/dom.rs::walk_event_for_id`, `packages/web/src/dom.rs::WebsysDom::new` |
| `onmounted` sees stale or absent node | Flush order | Mounted events drain after interpreter flush. `packages/web/src/mutations.rs::WebsysDom::flush_edits` |
| UI changes in VDOM but browser stays stale | Render and flush path | Main loop calls `render_immediate` then `flush_edits`. `packages/web/src/lib.rs::run` |
| Hydration requested without feature | Cargo features | Runtime panics when hydration chosen but `hydrate` feature absent. `packages/web/src/lib.rs::run` |

## 0.7 explanatory note

[0.7 Web guide](https://dioxuslabs.com/learn/0.7/guides/platforms/web) explains WASM target, `dioxus-web`, browser APIs, and custom `index.html`. Use local 0.8-alpha source above for launch signatures and missing-root behavior.
