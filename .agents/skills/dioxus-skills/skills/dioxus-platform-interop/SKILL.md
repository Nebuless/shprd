---
name: dioxus-platform-interop
description: Guides Dioxus platform escape hatches and foreign boundaries. Use when bridging Rust with JavaScript, DOM or Web Components, desktop webview IPC, native windows, Android activities, Swift or Kotlin through Manganis, WASM or TypeScript, React migrations, CSS tooling, or bundled assets.
metadata:
  invocation: model
---

# Dioxus platform interop

1. Verify `/root/repo/dioxus` is clean at `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`. If it differs, inspect changed symbols and refresh every affected claim before use.
2. Name runtime and boundary first: web WASM, desktop or mobile webview, LiveView, SSR, Dioxus Native, Android JNI, or Darwin ObjC. Do not present these as one interop facade.
3. Choose narrowest native Dioxus API that fits. Prefer RSX and typed events, then mounted element access, `document::eval`, renderer APIs, direct platform crates, and FFI in that order. Keep platform code behind Cargo features and target `cfg` gates.
4. Define boundary schema, ownership, thread, cancellation, cleanup, and failure behavior before writing bridge code. Pass structured serialized values across eval and FFI boundaries. Keep untrusted text out of executable JS and raw HTML.
5. Exercise boundary on each claimed target. Cover mount and unmount, repeated render, malformed payload, receiver drop or window close, and unavailable platform API. Finish only when cleanup runs and failures stay inside declared boundary.

## Branch references

- Read [`references/document-dom.md`](references/document-dom.md) for `dioxus_document`, eval channels, Web Components, mounted DOM access, and renderer differences.
- Read [`references/web-wasm.md`](references/web-wasm.md) for `web-sys`, `wasm-bindgen`, JavaScript or TypeScript modules, and WASM serialization.
- Read [`references/desktop-native.md`](references/desktop-native.md) for desktop webview IPC, `DesktopContext`, Wry integration, Dioxus Native windows, raw handles, and Android app access.
- Read [`references/manganis-ffi.md`](references/manganis-ffi.md) for Swift or Kotlin plugins, JNI or ObjC ownership, generated bindings, packaging, and current macro limits.
- Read [`references/react-css-assets.md`](references/react-css-assets.md) for React migration advice, Web Component coexistence, CSS and Tailwind, JavaScript assets, and asset lifecycle.
- Read [`references/source-map.md`](references/source-map.md) before citing support. Local source is authority. Official [escape hatch](https://dioxuslabs.com/learn/0.7/essentials/ui/escape) and [platform](https://dioxuslabs.com/learn/0.7/guides/platforms/) pages explain intent only.

## Boundary labels

- **Dioxus API** means symbol exists at pinned source revision and named path.
- **Renderer API** means public API from `dioxus-web`, `dioxus-desktop`, or `dioxus-native`, not portable core behavior.
- **Generic migration advice** means design guidance inferred from React, browser, WASM, JNI, or ObjC practice. Never label it Dioxus behavior.
- **Internal mechanism** means useful source evidence but no public contract. Avoid calling it from app code.
