# Mobile architecture

## Source pin

- Repository: `https://github.com/DioxusLabs/dioxus.git`
- Local checkout: `/root/repo/dioxus`
- Exact SHA: `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`
- Authority: local implementation first, same-SHA Markdown second, rendered 0.7 docs third.

## Crate shape

There is no standalone `dioxus-mobile` crate at this SHA. Mobile support is split across:

- `packages/dioxus/Cargo.toml:96-108`, where the public `mobile` feature enables `dioxus-desktop`.
- `packages/dioxus/src/lib.rs:107-117`, where `dioxus::mobile` is an alias reexport of `dioxus_desktop`, not a separate crate.
- `packages/desktop/Cargo.toml:1-8`, where `dioxus-desktop` identifies itself as the WebView renderer.
- `packages/desktop/src/lib.rs:8-45`, where the crate always includes private `mobile` code, conditionally includes mobile shortcuts for Android and iOS, and reexports Tao and Wry.
- `packages/cli/src/platform.rs:178`, where Android and iOS targets select the `mobile` feature group.
- `packages/cli/src/build/android.rs` and `packages/cli/src/build/apple.rs`, where `dx` creates platform packages.

Use `dioxus::launch(App)` or `LaunchBuilder` with the mobile target selected by `dx`. Do not add or recommend a `dioxus-mobile` dependency.

## Runtime stack

`dioxus-desktop` is the mobile renderer in this tree:

1. `packages/desktop/src/launch.rs:203-230` receives root component, root contexts, and platform config. It creates a `VirtualDom`, wraps ordinary launch in a default `Window`, then enters `launch_virtual_dom`.
2. `packages/desktop/src/launch.rs:60-147` creates `App` and runs Tao's event loop on the main thread. Tao owns native window and event dispatch.
3. `packages/desktop/src/webview.rs:253-563` creates a Tao window, then a Wry WebView. On mobile, no desktop default size is forced, allowing Tao to use screen size.
4. Wry loads `dioxus://index.html/`, carries DOM edits and events through IPC and custom protocols, and uses platform WebView engines. Dioxus renders HTML and CSS into that WebView.
5. `packages/desktop/src/app.rs:433-505` polls the `VirtualDom`, renders mutations, and flushes touched Wry queues.

The public dispatch is explicit: `packages/dioxus/src/launch.rs:338-346` sends both `KnownPlatform::Mobile` and `KnownPlatform::Desktop` to `dioxus_desktop::launch::launch`. Feature selection matters. `packages/dioxus/src/launch.rs:14-39` defines mobile as iOS and Android WebView, native as WGPU and Winit, and prioritizes native, desktop, then mobile when multiple renderer features are enabled. Avoid mixed renderer features because source warns about conflicts and binary bloat.

## Wry, Tao, and WGPU

- **Tao** owns native event loop, window, resize, close, and platform handles. Evidence: `packages/desktop/src/launch.rs:60-147` and `packages/desktop/src/webview.rs:279`.
- **Wry** owns WebView creation, IPC, custom protocols, URL loading, devtools, and platform WebView integration. Evidence: `packages/desktop/src/webview.rs:395-539` and `packages/desktop/Cargo.toml:31`.
- **WGPU** is not this WebView renderer. `packages/desktop/Cargo.toml` has no WGPU dependency. Workspace WGPU dependencies belong to native rendering or explicit integrations, including `packages/native/Cargo.toml` and `examples/10-integrations/wgpu-texture`.

When a mobile issue is HTML, CSS, JavaScript, navigation, IPC, or WebView security context, inspect Wry path. When it is native window or event-loop behavior, inspect Tao path. Reach for WGPU only when app explicitly embeds GPU rendering or uses native renderer code.

## Lifecycle boundary

Tao events drive Dioxus window lifecycle. `packages/desktop/src/launch.rs:73-89` handles initialization, event-loop destruction, close, destroy, resize, polls, new windows, and shutdown. This is not a full Android Activity or iOS application lifecycle API.

Android host lifecycle enters through Wry and Tao JNI callbacks. `packages/desktop/src/mobile.rs:5-8` records Wry 0.55 lifecycle methods on Kotlin `Rust`, not `WryActivity`. Activity recreation may call setup again. A `Once` protects NDK context initialization at lines 27-48.

Treat backgrounding, permission callbacks, intents, notifications, and platform service lifecycle as native-host concerns unless a specific Dioxus API is present. Keep UI state needed across host recreation in durable app state, not an assumed one-shot Activity instance.

## iOS host gap

Pinned source contains iOS runtime adaptations and UIKit access:

- `packages/desktop/src/desktop_context.rs:338-385` exposes current `UIView`, `UIViewController`, and main-thread-only view push/pop.
- `packages/desktop/src/webview.rs` includes iOS in mobile sizing, menu suppression, and Wry build paths.
- `packages/desktop/Cargo.toml:73-76` adds UIKit bindings.

Pinned source does not contain an iOS equivalent of Android `start_app`, a generated Swift application host, or an explicit Rust-to-iOS-host bootstrap contract. `packages/desktop/src/mobile.rs` only defines `start_app` under Android cfg. `packages/cli/assets/ios/` contains plist templating, not host source.

`packages/desktop/src/waker.rs:17-20` also records a known iOS event-loop gap: Wry lacks required support pending upstream issue 830, so the event-loop proxy wrapper uses unsafe `Send` and `Sync` declarations.

**Unknown:** exact UIKit application entrypoint and host glue used by a complete iOS launch are not established by this checkout. Do not infer symbol names, AppDelegate or SceneDelegate code, or Swift call order from Android. Verify generated output and dependency behavior on macOS before documenting or changing that boundary.

## Documentation status

The [official mobile guide](https://dioxuslabs.com/learn/0.7/guides/platforms/mobile), [platform overview](https://dioxuslabs.com/learn/0.7/guides/platforms/), and [desktop WebView guide](https://dioxuslabs.com/learn/0.7/guides/platforms/desktop) are useful for setup and command intent.

**Docs mismatch:** any wording that presents mobile as its own renderer or crate conflicts with this SHA's package layout. Source shows mobile paths inside `dioxus-desktop` and `dx`.

**Docs mismatch:** docs-level iOS launch confidence exceeds evidence in this checkout. Use docs as explanation, not proof of missing host implementation.
