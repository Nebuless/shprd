# Launch and assets

All claims use commit `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`.

## Launch decision table

| Situation | API | Constraint | Source path and symbol |
|---|---|---|---|
| One target selected by Cargo feature | `dioxus::launch(app)` | Root is `fn() -> Element`; delegates to `LaunchBuilder::new` | `packages/dioxus/src/launch.rs`, `launch` |
| Explicit web target | `LaunchBuilder::web()` | Compiled only with `web` | same file, `LaunchBuilder::web` |
| Explicit desktop target | `LaunchBuilder::desktop()` | Compiled only with `desktop` | same file, `LaunchBuilder::desktop` |
| Explicit mobile target | `LaunchBuilder::mobile()` | Compiled only with `mobile`; dispatches through desktop launch module | same file, `LaunchBuilder::mobile`, `LaunchBuilder::launch` |
| Explicit fullstack server | `LaunchBuilder::server()` | Requires `fullstack` and `server` | same file, `LaunchBuilder::server` |
| Third-party renderer | `LaunchBuilder::custom(fn)` | Launcher receives root, context factories, type-erased configs | same file, `LaunchBuilder::custom`, `LaunchFn` |
| Root dependency per launched app | `.with_context(value)` | Value is cloned by provider factory and must be `Any + Clone + Send + Sync + 'static` | same file, `LaunchBuilder::with_context` |
| Root dependency constructed on launch thread | `.with_context_provider(factory)` | Factory is `Send + Sync`; result is type-erased | same file, `LaunchBuilder::with_context_provider` |
| Platform config | `.with_cfg(config)` | Config implements `LaunchConfig`; renderer downcasts matching type | same file, `LaunchBuilder::with_cfg` |

`LaunchBuilder::new` chooses first enabled branch in this exact order: native, desktop, mobile, web, server, liveview. Multiple renderer features risk wrong platform and larger binaries. Explicit constructors avoid accidental selection for web, desktop, mobile, and server.

`LaunchBuilder::launch` dispatches native to `dioxus_native::launch_cfg`, mobile and desktop to `dioxus_desktop::launch::launch`, server to `dioxus_server::launch_cfg`, web to `dioxus_web::launch::launch`, and liveview to `dioxus_liveview::launch::launch`.

## Launch edge cases

- No supported renderer feature makes `LaunchBuilder::new` panic.
- `native` branch runs before desktop and mobile. Enabling native changes renderer selection.
- Fullstack native targets synthesize server URL from `DIOXUS_DEVSERVER_IP` and `DIOXUS_DEVSERVER_PORT` when unset. Web adds configured base path. See `LaunchBuilder::launch`.
- Configs are stored as `Box<dyn Any>`. Passing valid `LaunchConfig` does not prove selected renderer consumes that concrete type.
- Root props are not accepted by public `launch`. Wrap root props in a zero-argument root, inject context, or drive `VirtualDom::new_with_props` outside renderer launch.

## Asset decision table

| Need | API | Result | Source path and symbol |
|---|---|---|---|
| Required compile-time asset | `asset!(path)` | Compile error when macro cannot resolve input | `packages/manganis/manganis-macro/src/lib.rs`, `asset` |
| Optional compile-time asset | `option_asset!(path)` | `Option<Asset>` instead of missing-file compile error | same file, `option_asset` |
| Render asset URL | `Asset` in RSX attribute or `Display` | Calls `Asset::resolve` | `packages/manganis/manganis-core/src/asset.rs`, `Asset`, `Display` |
| Inspect source during unbundled development | `Asset::resolve` | Absolute source path when app is not bundled | same file, `Asset::resolve` |
| Read original file bytes | Keep source path separately | `Asset` bundled path is not direct file read contract | same file, `BundledAsset::bundled_path`, `Asset` rustdoc |

Path rules come from macro source: `.` and `..` start paths relative to current Rust file; other paths resolve from crate root; leading slash is ignored; `OUT_DIR` paths remain full paths.

Bundled mode resolves under optional base path plus `/assets/`, joined with generated bundled path. CLI must patch linker asset metadata. Placeholder hash, null pointer, or deserialization failure indicates missing or mismatched `dx` processing. See `BundledAsset::PLACEHOLDER_HASH` and `Asset::bundled`.

## Verification

1. Compile each intended target feature separately.
2. Confirm selected `LaunchBuilder` constructor or feature branch.
3. For assets, run through matching `dx` build or serve path, not plain unit test alone.
4. Assert rendered asset attribute uses bundled URL. Test direct file I/O against explicit source path, not `Asset::to_string()`.

Official docs: [getting started](https://dioxuslabs.com/learn/0.7/getting_started/), [platform guides](https://dioxuslabs.com/learn/0.7/guides/), [assets tutorial](https://dioxuslabs.com/learn/0.7/tutorial/assets). Source above decides exact behavior.
