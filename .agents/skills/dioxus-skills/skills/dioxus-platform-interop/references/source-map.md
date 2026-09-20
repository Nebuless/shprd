# Source map and gaps

## Pin

- Repository: `https://github.com/DioxusLabs/dioxus.git`
- Local checkout: `/root/repo/dioxus`
- Exact SHA: `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`
- Authority: local implementation at pin. Official docs explain intent and can lag symbols.

## Document and eval

- `packages/document/src/lib.rs`: `document`, `eval`
- `packages/document/src/document.rs`: `Document`, `NoOpDocument`, `create_element_in_head`
- `packages/document/src/eval.rs`: `Eval`, `Evaluator`, `send`, `recv`, `join`, `IntoFuture`
- `packages/document/src/error.rs`: `EvalError`
- `packages/document/docs/eval.md`: security, channels, DOM lifecycle examples
- `packages/web/src/document.rs`: `WebDocument`, `WebEvaluator`, async wrapper, JSON conversion
- `packages/web/src/ts/eval.ts`: web channel queue and close behavior, internal source
- `packages/web/src/js/eval.js`: generated web channel module consumed by Rust
- `packages/desktop/src/document.rs`: `DesktopDocument`, `DesktopEvaluator`
- `packages/desktop/src/ts/native_eval.ts`: native query channel, internal source
- `packages/desktop/src/js/native_eval.js`: generated native query module consumed by bootstrap
- `packages/liveview/src/document.rs`: LiveView document provider
- `packages/fullstack-server/src/document.rs`: server no-op eval

## DOM and Web Components

- `packages/core-macro/docs/rsx.md`: dashed Web Component tags and typed wrapper example
- `packages/rsx/src/node.rs`, `packages/rsx/src/rsx_block.rs`: dashed-tag parser, internal
- `packages/html/src/lib.rs`: custom element namespace support
- `packages/html/src/events/generated.rs`: mounted event guidance
- `packages/web/src/events/mod.rs`: `WebEventExt`, event conversion, mounted feature failure
- `packages/web/src/events/mounted.rs`: `MountedData` to `web_sys::Element`
- `packages/web/src/dom.rs`, `packages/web/src/mutations.rs`: generic DOM element creation, event delegation, mount flush ordering
- `packages/hooks/docs/side_effects.md`: post-render DOM effects

## Desktop and native

- `packages/desktop/src/lib.rs`: public exports
- `packages/desktop/src/hooks.rs`: `use_window`, `use_app`, `use_wry_event_handler`, `use_asset_handler`
- `packages/desktop/src/desktop_context.rs`: `window`, `app`, `DesktopContext`, `DesktopService`, `PendingDesktopWindow`
- `packages/desktop/src/desktop_state.rs`: app and window contexts
- `packages/desktop/src/protocol.rs`: internal custom protocol and bootstrap injection
- `packages/desktop/src/query.rs`, `ipc.rs`, `webview.rs`, `launch.rs`: internal IPC flow
- `packages/native/src/lib.rs`: Native `use_window`, `use_raw_window_handle`, Android app setters and getters
- `packages/desktop/src/mobile.rs`: Android desktop/webview startup and `ndk_context` setup

## Android and Manganis FFI

- `packages/manganis/manganis/src/lib.rs`: public `ffi`, assets, target reexports
- `packages/manganis/manganis/src/android/activity.rs`: `with_activity`, cached VM and Activity
- `packages/manganis/manganis-macro/src/lib.rs`: `ffi` docs and macro entry
- `packages/manganis/manganis-macro/src/ffi.rs`: parser, JNI and ObjC code generation
- `packages/manganis/manganis/src/android/callback.rs`: low-level DEX callback bridge, internal and unsafe
- `packages/manganis/manganis-core/src/ffi.rs`: `SymbolData`, `AndroidArtifactMetadata`, `SwiftPackageMetadata`
- `packages/cli/src/build/request.rs`: metadata extraction and native artifact staging
- `packages/cli/src/build/android.rs`: Gradle plugin artifact installation
- `examples/01-app-demos/geolocation-native-plugin/src/plugin/mod.rs`: target-gated Swift and Kotlin declarations plus fallback

## CSS and assets

- `packages/manganis/manganis/README.md`: asset and JS behavior
- `packages/manganis/manganis-core/src/asset.rs`: `Asset`
- `packages/manganis/manganis-core/src/options.rs`: `AssetOptions`, variants
- `packages/manganis/manganis-core/src/css.rs`: CSS options
- `packages/manganis/manganis-core/src/css_module.rs`: CSS module options
- `packages/manganis/manganis-core/src/js.rs`: JS module, minify, preload, static-head options
- `packages/manganis/manganis-macro/src/css_module.rs`: generated scoped classes and stylesheet insertion
- `packages/asset-resolver`: portable asset URL, bytes, and native path resolution

## Official explanatory docs

- [Escape hatches](https://dioxuslabs.com/learn/0.7/essentials/ui/escape): custom attributes, raw HTML, Web Components, eval, web-sys, DOM access, overlays, Tauri, native widgets, Dioxus Native.
- [Platform support](https://dioxuslabs.com/learn/0.7/guides/platforms/): Cargo feature and dependency gating.
- [Web](https://dioxuslabs.com/learn/0.7/guides/platforms/web): WASM, wasm-bindgen, eval, custom index.
- [Desktop](https://dioxuslabs.com/learn/0.7/guides/platforms/desktop): native Rust plus system webview, eval, assets, Wry.
- [Mobile](https://dioxuslabs.com/learn/0.7/guides/platforms/mobile): webview or experimental WGPU, Android and iOS setup.
- [Assets](https://dioxuslabs.com/learn/0.7/essentials/ui/assets): linker metadata, hashing, folders, resolver, public folder.
- [Styling](https://dioxuslabs.com/learn/0.7/essentials/ui/styling): CSS, document stylesheets, SCSS, Tailwind.

## Known gaps

- No public single interop facade exists across DOM, desktop, native renderer, Android, Swift, and Kotlin. This skill intentionally routes among distinct APIs.
- Official pages still say `use_eval` in some prose while pinned public call is `document::eval`.
- Manganis FFI has little stable public documentation and alpha package version at pin. Source is required for signatures and packaging behavior.
- Pinned Android `with_activity` caches first Activity globally and exposes no recreation refresh.
- Desktop internal IPC isn't public app API. Custom IPC advice depends on Wry public surface selected by app configuration.
- React migration and coexistence guidance is generic advice. Pinned Dioxus source exposes Web Components and DOM bridges, not React adapter.
- TypeScript compilation pipeline isn't supplied by Manganis. It processes emitted JavaScript.
- Dioxus CLI disables wasm-bindgen-generated TypeScript declarations at this pin.
- Web Component support is generic dashed-tag and DOM behavior; no dedicated shadow DOM lifecycle or property-reflection facade exists.
- Browser automation docs could not be opened through local Playwright because Chrome distribution was absent; official pages were fetched as rendered Markdown instead.
