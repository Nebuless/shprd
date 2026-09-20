# React migration, CSS, and assets

## Authority split

Following items are **generic React migration advice**, not Dioxus native APIs:

- Port component boundaries, props, keys, controlled inputs, and state ownership conceptually. Rewrite JSX and hooks in Rust and RSX instead of transliterating syntax.
- Replace React state with Dioxus signals and resources according to Dioxus semantics. Do not assume React effect dependency arrays, synthetic event pooling, context identity, refs, portals, Suspense, or concurrent rendering map one-to-one.
- Migrate vertical slices. Keep old React island behind Web Component or separate route, define JSON or attribute contract, then replace island. Directly mounting React into Dioxus-owned children creates two reconcilers competing for DOM.
- If React must coexist in one page, give each runtime separate root. Share browser events or a typed message bus, not framework internals.

Dioxus APIs used during migration include RSX Web Components, custom quoted attributes, `document::eval`, `WebEventExt`, mounted elements, document head components, and Manganis assets. React itself is not embedded or translated by Dioxus.

## React interoperation choices

- Existing standards-based Web Component: render dashed tag in RSX and wrap it in typed Dioxus component.
- Existing React component only: expose it as Web Component or mount it into empty host from JS module after mount. Destroy React root on cleanup.
- Shared design system: reuse CSS tokens, fonts, icons, and custom elements first. Port stateful components later.
- Shared TypeScript models: convert to explicit wire schemas. Rust and TypeScript types don't validate each other at runtime.
- Whole React app: route-level coexistence usually beats per-component embedding. It isolates bundlers, routers, global CSS, and ownership.

Test event bubbling across shadow roots, focus, forms, hydration, router history, CSS reset conflicts, and double initialization under hot reload.

## CSS and tooling

- Dioxus first-party renderers use HTML and CSS concepts. Third-party renderers may differ.
- Prefer stylesheet assets and classes over repeated inline strings. `document::Stylesheet` participates in document head and SSR preload behavior.
- CSS custom properties are good interop contract for themes and Web Components. Scope resets around embedded React or third-party widgets.
- DX 0.7 can detect root `tailwind.css`, run Tailwind watcher, and emit `assets/tailwind.css`. Include output with `document::Stylesheet` and `asset!`.
- Tailwind scans Rust only when source directive covers Rust files. Dynamic class fragments may escape static scanner; use complete class strings or safelist supported by current Tailwind setup.
- Manganis `#[css_module]` generates scoped class accessors and inserts stylesheet. This is Dioxus tooling, separate from generic CSS Modules in React bundlers.

## Assets

- `asset!` emits linker metadata consumed by Dioxus CLI. It does not embed file bytes like `include_bytes!`.
- Returned `Asset` must remain used or linker can remove metadata. `#[used]` is escape hatch for indirectly referenced assets.
- Paths resolve from package root conventions, then CLI hashes and processes output. Never assume source filesystem path at runtime.
- Use `dioxus::asset_resolver::read_asset_bytes` for portable reads. `asset_path` does not work for web or Android bundles.
- Folder asset output name is hashed. Build child URL from formatted returned folder asset, not source folder literal.
- `option_asset!` handles optional source. It does not turn failed network or runtime loads into success.

## JavaScript assets

- `.js` auto-detection distinguishes classic and ESM. `.mjs` always ESM; `.cjs` classic.
- Minification defaults on. `with_minify(false)` preserves prebuilt vendor output. ESM local imports can be bundled; HTTP imports remain runtime imports.
- `with_static_head(true)` emits script tag; `with_preload(true)` emits preload; `with_module(true)` forces ESM.
- Static head load is global and order-sensitive. For component-scoped library, explicit load plus readiness promise can be safer.
- Check CSP. Eval requires policy allowances that external or hashed modules may avoid. Asset URL trust does not make script content safe.

## Asset and style lifecycle

- Document head components should own app-global styles and scripts. Component-specific imperative insertion needs idempotence and cleanup.
- Hot reload can rerun registration. Custom elements cannot be defined twice; guard `customElements.get(name)` in trusted module.
- Preload only critical assets. Browser warns about unused preload and wrong `as` type.
- Verify production hashes, base path, MIME types, module imports, offline behavior, and cache invalidation.
