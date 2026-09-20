# Platform integration

All facts below are pinned to Dioxus `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26` in `/root/repo/dioxus`.

## Permissions have two phases

1. Declare capability for package metadata.
2. Request access at runtime through platform API or native plugin.

`packages/cli/src/config/manifest.rs:23-110` defines unified permission declarations. `packages/cli/src/config/manifest_mapper.rs:61-123` maps them to Android permissions and Apple plist entries. Android generation consumes mappings in `packages/cli/src/build/android.rs:153-199`; Apple plist generation consumes them in `packages/cli/src/build/apple.rs:147-279`.

Manifest or plist declaration alone does not grant runtime access. Dioxus source here does not provide one universal runtime permission requester. Use browser APIs only when target WebView exposes required API and secure-context rules permit it. Otherwise use Kotlin/Java or Swift/Objective-C integration, then return typed results to Rust.

On Android, `packages/desktop/src/webview.rs:432-438` enables HTTPS scheme behavior for Dioxus custom protocol so secure-context APIs such as geolocation can work. This does not replace Android permission declaration or runtime grant.

Use raw platform sections only when unified mapping lacks capability:

- Android config fields, dependencies, plugins, ProGuard, raw manifest, and extra permissions: `packages/cli/src/config/manifest.rs:592-664`.
- iOS plist, entitlements, background modes, document types, and raw XML: `packages/cli/src/config/manifest.rs:328-378` and `469-517`.

## Android bootstrap and JNI

Generated host is intentionally fixed:

- `packages/cli/assets/android/MainActivity.kt.hbs:1-5` declares package `dev.dioxus.main` and subclasses `WryActivity`.
- `packages/desktop/src/mobile.rs:1-15` exports `start_app`, whose body expands Tao and Wry Android bindings.
- `packages/desktop/src/mobile.rs:54-55` binds package segments `dev_dioxus`, `main`, and Kotlin object `Rust`.
- `packages/desktop/src/mobile.rs:69-99` resolves exported `main` or `_main` with `dlsym`, loads debug environment values, and calls Rust entrypoint. Panic crossing JNI boundary is caught and process aborts.

Top-level Android application ID may differ. Host JNI package remains `dev.dioxus.main`; generated Kotlin uses a `BuildConfig` typealias for configured application ID. Changing package, Kotlin host, or Wry version requires matching JNI symbols. `UnsatisfiedLinkError` at startup usually means host symbols, package/object name, or loaded library no longer agree.

NDK context initialization is process-global. `packages/desktop/src/mobile.rs:20-50` initializes it once before Wry setup because Tao 0.35 no longer does so. Activity recreation can reenter setup, but must not reinitialize global NDK context.

## Android native plugins

Native Android plugin metadata is extracted from linked artifacts, then installed before Gradle assembly:

- `packages/cli/src/build/request.rs:2048-2069` selects Android artifacts and calls installer.
- `packages/cli/src/build/android.rs:443-540` accepts source directories as Gradle submodules or `.aar` files under `app/libs`, then adds plugin Gradle dependencies.
- `packages/cli/src/config/manifest.rs:618-624` allows app-level extra Gradle dependencies and plugins.

Keep FFI seam narrow. Rust owns typed request and response shapes. Kotlin owns Activity, Context, permission callback, Intent, and SDK APIs. Confirm thread requirements before calling UI APIs, and marshal callbacks back to event-loop-safe Rust code.

Do not describe source-folder or AAR installation as direct JNI generation. Packaging and call ABI are separate contracts. `packages/manganis/manganis-macro/src/ffi.rs:1-22,32-145` proves that `#[manganis::ffi]` parses an `extern "Kotlin"` bridge and generates JNI signatures for supported primitive, string, option, result, opaque-reference, and unit forms. Later macro code performs class lookup from plugin namespace, constructs plugin with an Activity, invokes generated JNI methods, and emits `AndroidArtifactMetadata`. Use the geolocation plugin under `examples/01-app-demos/geolocation-native-plugin` as checked-in usage, but inspect generated signatures before changing native methods.

## Swift plugins

Apple native plugins use metadata embedded in linked artifacts:

- `packages/cli/src/build/assets.rs:533-560` extracts `SymbolData`, including Swift package metadata.
- `packages/cli/src/build/request.rs:2071-2090` compiles Swift packages for iOS or macOS and embeds Swift standard libraries.
- `packages/cli/src/build/apple.rs:1042-1240` copies each package, changes library products to dynamic, runs `xcrun swift build`, finds dylibs, and wraps them as frameworks.
- `packages/cli/src/build/apple.rs:866-954` installs framework bundle and uses `xcrun swift-stdlib-tool`.

This proves build and embedding path, not call ABI. Inspect plugin macro and package source before asserting class names, symbol names, callback queues, object retention, or error representation.

For macro-backed plugins, `packages/manganis/manganis-macro/src/ffi.rs` also proves an `extern "Swift"` path using Objective-C runtime class lookup, selector dispatch, and `dlopen` of bundled `DioxusSwiftPlugins.framework`. Swift classes and methods must therefore be Objective-C visible. Supported Rust declaration shapes are the same parser-limited set above; unsupported types are rejected. This still does not establish each plugin's class name, selector spelling, threading, retention, or asynchronous callback semantics.

UIKit access exposed by Dioxus is main-thread constrained. `DesktopContext::push_view` and `pop_view` assert main thread in `packages/desktop/src/desktop_context.rs:356-385`.

## iOS host unknowns

Known: `dx` builds an iOS `.app`, writes `Info.plist`, supports signing, embeds frameworks, and can package signed device app as IPA.

Unknown at pinned source: generated AppDelegate or SceneDelegate, Swift host source, iOS counterpart to Android `start_app`, and exact process entry into `dioxus_desktop::launch`. Do not fabricate any of these. If feature needs host edits, first inspect actual generated `.app`, linker symbols, and upstream Wry/Tao version sources available in build environment.

## Documentation status

Official 0.7 platform pages explain intended setup and high-level use.

**Docs mismatch:** runtime permission behavior is not proven by package metadata examples. Source only proves declaration and mapping.

**Docs mismatch:** native plugin packaging does not itself prove FFI call shape. Keep build path and ABI path distinct.
