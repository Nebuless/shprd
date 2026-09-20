# Browser integration

## History and routing

Renderer initializes document and default history only when no context already exists. Client-only default is `WebHistory`; hydrated fullstack uses fullstack wrappers around `WebHistory`. Pins: `packages/web/src/document.rs::init_document_with`, `packages/web/src/document.rs::init_document`, `packages/web/src/document.rs::init_fullstack_document`.

Choose history:

- `WebHistory` reads pathname, query, and hash; strips configured prefix; writes browser History API state; listens to `popstate`; optionally stores and restores scroll. Prefix falls back to CLI web base path and is normalized to one leading slash with no trailing slash. Pins: `packages/web/src/history.rs::WebHistory::new_inner`, `packages/web/src/history.rs::impl History for WebHistory`.
- `HashHistory` stores route after `#` while retaining current pathname. It supports single-file or single-path hosting without server route rewrites. Pins: `packages/web/src/history.rs::HashHistory`, `packages/web/src/history.rs::impl History for HashHistory`.
- `Config::history` injects custom provider before document init. `HistoryProvider` can provide one to child routers, and renderer defaults only fill missing context. Pins: `packages/web/src/cfg.rs::Config::history`, `packages/router/src/components/history_provider.rs::HistoryProvider`, `packages/web/src/document.rs::init_document_with`.

For base-path apps, align three values: CLI `[web.app].base_path`, browser history prefix, and host mount path. Dioxus launch also appends CLI base path to fullstack server-function URL on web. Pins: `packages/cli/src/config/web.rs::WebAppConfig`, `packages/web/src/history.rs::WebHistory::new_inner`, `packages/dioxus/src/launch.rs::LaunchBuilder::launch`.

## Document head

`WebDocument` provides eval and queues title/head mutations as effects. Meta, script, style, and link elements append to `document.head`; browser errors propagate only inside helper result, while callers discard result. Pins: `packages/web/src/document.rs::impl Document for WebDocument`, `packages/web/src/document.rs::append_element_to_head`.

Use Dioxus `document::*` components for declarative head changes. Test actual head after render because operations are effect-queued. Existing browser fixture covers title, meta, link, stylesheet, script, and style in `packages/playwright-tests/web/src/main.rs::DocumentElements`.

## JavaScript eval

`document::eval(script)` dispatches through current `Document`. Web implementation:

1. Creates JS channel and weak Rust channel.
2. Wraps script in async function so WASM thread remains available for channel traffic.
3. Executes with `Function`, resolves returned value as promise, JSON-stringifies result, then parses into `serde_json::Value`.
4. Supports Rust to JS `send`, JS to Rust `dioxus.send`, JS receive through `await dioxus.recv()`, and awaiting final result.

Pins: `packages/document/src/lib.rs::eval`, `packages/web/src/document.rs::WebEvaluator::create`, `packages/web/src/document.rs::PROMISE_WRAPPER`, `packages/web/src/document.rs::impl Evaluator for WebEvaluator`.

Safety and failure rules:

- Script has page privileges. Pass trusted JavaScript only. 0.7 [eval docs](https://dioxuslabs.com/learn/0.7/essentials/ui/escape) explain XSS risk; local execution mechanism is pinned above.
- Values crossing channel or final result must serialize through `serde_wasm_bindgen` or JSON path. Check `EvalError::Communication` or serialization failures instead of assuming arbitrary JS objects cross intact. Pins: `packages/web/src/document.rs::WebEvaluator::create`, `packages/document/src/error.rs::EvalError`.
- Keep eval alive while exchanging messages. Dropped channel owner ends backing generational storage. Pin: `packages/web/src/document.rs::JSOwner`.

Browser fixture for return value and ten send/receive exchanges: `packages/playwright-tests/web/src/main.rs::app`.

## Direct browser APIs

Use `web-sys`, `js-sys`, or `wasm-bindgen` when API is web-specific. Use effects for DOM reads or listener installation after render. Runtime can cross into a `wasm_bindgen::Closure` through Dioxus callback. Fixture: `packages/playwright-tests/web/src/main.rs::WebSysClosure`.

Keep closure alive as long as browser listener can call it. The fixture uses `Closure::forget`, which intentionally leaks for app lifetime. For removable listeners, retain closure and unregister before drop.

Web event data supports downcast to native web event through `WebEventExt`; invalid type panics in strict accessor. Pin: `packages/web/src/events/mod.rs::WebEventExt`.

## Files and browser boundaries

Web file data wraps browser `File` and reads bytes through `FileReader`; file-list conversion produces cross-platform `FileData` values. Pins: `packages/web/src/files.rs::WebFileData`, `packages/web/src/files.rs::WebFileEngine::to_files`.

Browser fullstack client can't stream request bodies through Fetch in current path, so client collects stream before request. Pin: `packages/fullstack/src/client.rs::ClientRequest::send_body_stream`.

## Troubleshooting

| Symptom | Check | Pin |
|---|---|---|
| Direct route refresh returns host 404 | SPA fallback or HashHistory | `packages/web/src/history.rs::HashHistory`, `packages/cli/src/serve/server.rs::no_cache` |
| Base-path links duplicate or lose prefix | CLI base path and explicit history prefix | `packages/web/src/history.rs::WebHistory::new_inner` |
| Back navigation route changes but scroll doesn't | `do_scroll_restoration`, state written before push, popstate callback | `packages/web/src/history.rs::impl History for WebHistory` |
| Head element missing immediately after render call | Effect scheduling | `packages/web/src/document.rs::impl Document for WebDocument` |
| Eval hangs | Script awaits receive with no Rust send, or Rust awaits receive/final result never produced | `packages/web/src/document.rs::PROMISE_WRAPPER`, `packages/web/src/document.rs::impl Evaluator for WebEvaluator` |
| Eval result fails despite JS success | `undefined`, invalid UTF-16, or non-JSON result | `packages/web/src/document.rs::WebEvaluator::create` |
| Browser listener stops firing | Dropped `Closure` | `packages/playwright-tests/web/src/main.rs::WebSysClosure` |
