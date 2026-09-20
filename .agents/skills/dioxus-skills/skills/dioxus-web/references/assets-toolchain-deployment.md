# Assets, WASM toolchain, testing, and deployment

## Toolchain path

Use Dioxus CLI for normal web work:

1. `dx serve --web` for client-only development. CLI platform alias selects `wasm32-unknown-unknown`, web renderer, and web bundle format. Pins: `packages/cli/src/platform.rs::Platform::Web`, `packages/cli/src/build/request.rs::BuildRequest::new`.
2. `dx build --web --release` for production-shaped client bundle. Fullstack build produces separate client WASM and server binary according to selected targets. Pin: `packages/cli/src/build/request.rs::BuildRequest::new`.
3. Let CLI verify matching `wasm-bindgen` version from workspace and obtain esbuild. Pin: `packages/cli/src/build/web.rs::BuildRequest::verify_web_tooling`.
4. CLI runs `wasm-bindgen --target web`, optional splitting, release `wasm-opt`, asset registration, JS glue, then writes `index.html`. Pin: `packages/cli/src/build/web.rs::BuildRequest::bundle_web`.

Don't diagnose a raw `cargo build` artifact as deployable web output. Browser needs generated JS glue, WASM URL, assets, and HTML assembly.

## Assets

Use `asset!("/path")` for files known at compile time. CLI extracts emitted metadata, hashes or processes files, registers bundle paths, and injects preloads or resource tags where options require. Pins: `packages/cli/src/opt/mod.rs::AppManifest::register_asset`, `packages/cli/src/build/web.rs::BuildRequest::prepare_html`.

For runtime bytes:

- `read_asset_bytes` uses web resolver under `web` feature and Fetches URI. Pin: `packages/asset-resolver/src/lib.rs::read_asset_bytes`.
- Web resolver normalizes missing leading slash, Fetches request, converts response `array_buffer` to bytes. It doesn't check HTTP status before reading body at this SHA. Pin: `packages/asset-resolver/src/web.rs::resolve_web_asset`.
- `asset_path` can't represent web asset as filesystem path and returns `CannotRepresentAsPath`. Pin: `packages/asset-resolver/src/lib.rs::asset_path`.

When asset 404s, inspect emitted URL, configured base path, generated `index.html`, network response, and host path. Don't replace `asset!` URL with local filesystem path.

## Bundle and HTML

CLI web bundle layout is documented in `packages/cli/src/build/web.rs` module docs: web root, optional server, public `index.html`, WASM glue, snippets, and assets. Exact hashed placement depends on build mode.

`prepare_html` chooses custom project `index.html` when present, otherwise dev or production template; injects resources, base-path metadata, module loader, and dev-only pieces. Pin: `packages/cli/src/build/web.rs::BuildRequest::prepare_html`.

Release path optimizes main WASM when release or split build, registers main WASM and JS as assets when bundle format requires, and writes JS loader using base path. Pin: `packages/cli/src/build/web.rs::BuildRequest::bundle_web`.

Web config source:

- `[web.app]`: title and optional `base_path`.
- `[web.watcher]`: watched paths, HTML reload, and dev `index_on_404`, default true.
- `[web.resource]`: dev and general style/script resources.
- `[web.https]`: enablement and certificate options.
- `[web.wasm_opt]`: optimization level, debug info, names, memory packing, and enable/disable feature flags.

Pin: `packages/cli/src/config/web.rs::WebConfig` and nested config structs. Current code default `WasmOptLevel` is `z`, despite stale field comment mentioning `s`; enum implementation wins. Pin: `packages/cli/src/config/web.rs::WasmOptLevel`.

## Browser test workflow

1. Start app through same `dx serve` mode users run.
2. Drive browser with Playwright or equivalent. Assert visible result, URL/history, head, console, and network as applicable.
3. Exercise one update after initial render or hydration. Renderer binding bugs often appear only on second mutation.
4. Repeat production-shaped release bundle under static HTTP server. Confirm direct route loads, base path, MIME types, JS glue, WASM, and assets.
5. For hydration, cover both initial HTML and streamed suspense update. Use edge list in `references/hydration-ssr.md`.

Pinned upstream fixtures:

- `packages/playwright-tests/web/src/main.rs::app`: rendering, attributes, eval, prevent-default, mounted, direct browser closure, document head.
- `packages/playwright-tests/web-routing`: browser route behavior.
- `packages/playwright-tests/web-hash-routing`: hash route behavior.
- `packages/playwright-tests/fullstack-routing`: hydrated routing.
- `packages/playwright-tests/markerless-hydration-edges/src/main.rs::app`: hydration edge matrix.
- `packages/playwright-tests/cli-optimization/src/lib.rs`: asset processing variants.

[0.7 web testing](https://dioxuslabs.com/learn/0.7/guides/testing/web) is explanatory. Keep test commands compatible with local workspace and source fixtures.

## Deployment workflow

Client-only SPA:

1. Build release web bundle.
2. Upload generated web output, not Cargo target WASM alone.
3. Serve `.wasm` with correct WASM MIME type and JS as JavaScript.
4. Configure fallback from unknown application paths to `index.html` when using `WebHistory`. Use `HashHistory` when host can't rewrite.
5. If deployed below domain root, set base path during build and verify all module, WASM, split chunk, asset, route, and server-function URLs.

Fullstack:

1. Deploy server binary plus generated web/public output expected by bundle.
2. Keep server HTML, hydration payload, and client WASM from same build.
3. Preserve streaming responses when HTML streaming is enabled. Proxy buffering can hide chunks even when server emits them.
4. Smoke test initial SSR without client execution, then hydration and navigation with client execution.

Pins: `packages/cli/src/build/web.rs::BuildRequest::bundle_web`, `packages/web/src/lib.rs::run`, `packages/fullstack-server/src/streaming.rs::StreamingRenderer::replace_placeholder`.

## Failure table

| Symptom | Inspect | Source pin |
|---|---|---|
| `wasm-bindgen` version error | Workspace dependency and CLI-selected binary | `packages/cli/src/build/web.rs::BuildRequest::verify_web_tooling`, `packages/cli/src/wasm_bindgen.rs::WasmBindgen::verify_install` |
| Browser fetches wrong WASM or chunk path | Build base path and generated glue | `packages/cli/src/build/web.rs::BuildRequest::write_js_glue_shim`, `BuildRequest::bundle_web` |
| Dev navigation works, production refresh 404s | Dev `index_on_404` hid missing host rewrite | `packages/cli/src/config/web.rs::WebWatcherConfig`, `packages/cli/src/serve/server.rs::no_cache` |
| Asset bytes returned for HTTP 404 page | Resolver doesn't inspect response status | `packages/asset-resolver/src/web.rs::resolve_web_asset` |
| Release stack traces unreadable | `wasm_opt.keep_names` or debug settings | `packages/cli/src/config/web.rs::WasmOptConfig` |
| Custom `index.html` blank | Missing configured mount element or loader injection assumptions | `packages/cli/src/build/web.rs::BuildRequest::prepare_html`, `packages/web/src/dom.rs::WebsysDom::new` |
| WASM crate fails to compile | Native syscall or target-incompatible dependency | [0.7 Web guide](https://dioxuslabs.com/learn/0.7/guides/platforms/web) explains WASM limits; Cargo target errors are final evidence |

## 0.7 explanatory note

[0.7 Web guide](https://dioxuslabs.com/learn/0.7/guides/platforms/web) explains WASM and custom HTML. [0.7 deployment](https://dioxuslabs.com/learn/0.7/tutorial/deploy) explains publishing flow. Local 0.8-alpha CLI source above decides command behavior, defaults, generated paths, and optimization pipeline.
