# Web, WASM, JavaScript, and TypeScript

## Pick boundary

1. Use RSX, typed events, and `MountedData` for portable UI behavior.
2. Use `document::eval` for small renderer-portable JavaScript snippets and JSON messages.
3. Use `web-sys`, `js-sys`, and `wasm-bindgen` for web-only typed browser APIs.
4. Use a JavaScript or TypeScript module when browser-side state, callbacks, or an existing package should remain in JS.

Steps 3 and 4 are web renderer choices. Official escape-hatch docs explicitly warn that `web-sys` is not portable to Dioxus desktop or mobile webview apps, where Rust runs natively rather than as WASM.

## wasm-bindgen contract

- Put browser-only modules and dependencies behind a Cargo feature such as `web`, and combine it with `#[cfg(target_arch = "wasm32")]` when target architecture matters.
- Declare narrow imported functions and exported callbacks. Keep `JsValue` at boundary; convert immediately to typed Rust or JS data.
- Use `serde-wasm-bindgen` for structured values when direct bindings are not practical. Document JS names, nullability, thrown exceptions, and promise rejection.
- Enable required `web-sys` feature flags for every browser type and method. Missing feature-generated methods are dependency configuration, not runtime absence.
- Browser APIs may require secure context, permission, transient user activation, or main-thread execution. Model denial and unsupported browsers as ordinary outcomes.

## TypeScript

- TypeScript declarations improve JS-side checking but don't validate runtime payloads. Parse untrusted or versioned messages at boundary on both sides.
- Compile TypeScript to JS before Dioxus asset processing. Manganis handles JavaScript assets, not TypeScript source compilation.
- Dioxus CLI invokes `wasm-bindgen` with generated TypeScript disabled at this pin. Supply and version consumer declarations yourself when exposing a TS-facing module.
- Export a small stable module API. Bind that API with `#[wasm_bindgen(module = "...")]`; do not mirror a whole npm package into Rust.
- Preserve ESM shape. Manganis detects top-level `import`, `export`, or `import.meta`; `.mjs` forces module and `.cjs` forces classic script. `AssetOptions::js().with_module(true)` handles side-effect-only ESM.
- Keep generated `.d.ts`, JS bundle, and Rust extern declarations on one version. Integration test one call in each direction and one rejected promise.

## Data and callbacks

- Use explicit DTOs for boundary data. JSON-compatible values exclude cycles, `BigInt`, functions, symbols, DOM objects, and most class identity.
- Preserve integer limits. JavaScript numbers cannot exactly represent every Rust 64-bit integer. Encode large IDs as strings or use explicit `BigInt` bindings.
- Give callbacks a removal path. JS listeners holding WASM closures can leak; Rust closures dropped while JS still calls them can fail. Store closure ownership beside registration and unregister before drop.
- Avoid synchronous reentry into mutable Rust state. Queue state updates through Dioxus events or tasks when external JS can call during rendering.
- Cancel timers, observers, subscriptions, and pending work on component cleanup. Guard async completion against owner unmount.

## DOM ownership

- Let Dioxus own RSX subtree. Give imperative library one empty host element and let library own descendants.
- Initialize after `onmounted` or `use_effect`; update through library API rather than rebuilding on each render; destroy during cleanup.
- For element identity, use mounted element handle where available. Generated unique IDs plus `getElementById` are acceptable across eval, but escape IDs before embedding and prefer message channels over source formatting.
- Hydration requires server and first client DOM to match. Delay browser-only mutation until hydration completes.

## Verification

- Build web target, load real browser, and inspect console.
- Test JS exception, promise rejection, malformed payload, owner unmount, repeated mount, and browser API denial.
- Test release asset output, not only dev server. Module paths, CSP, base paths, and minification differ.
