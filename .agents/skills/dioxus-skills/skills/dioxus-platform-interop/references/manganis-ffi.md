# Manganis Swift and Kotlin FFI

## Status and scope

`#[manganis::ffi]` exists at pinned source and generates direct JNI or ObjC bindings plus linker metadata. It is not one portable runtime facade. Generated implementations differ by `extern "Kotlin"` and `extern "Swift"`, target `cfg`, package layout, loader, and ownership model.

Use it when native source belongs with Rust crate and Dioxus CLI owns mobile packaging. For broad third-party SDKs, callbacks, async streams, or unsupported types, a hand-written narrow adapter inside Swift or Kotlin often gives cleaner ABI.

## Declaration

```rust
#[cfg(target_os = "android")]
#[manganis::ffi("src/android")]
unsafe extern "Kotlin" {
    pub type NativePlugin;
    pub fn readValue(this: &NativePlugin, input: String) -> String;
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[manganis::ffi("src/ios/plugin")]
unsafe extern "Swift" {
    pub type NativePlugin;
    pub fn readValue(this: &NativePlugin, input: String) -> String;
}
```

- Path is relative to `CARGO_MANIFEST_DIR`.
- Swift path points to SwiftPM package containing `Package.swift`.
- Kotlin path points to Gradle project containing `build.gradle.kts`; macro reads namespace and plugin metadata.
- Opaque `type Name;` becomes Rust wrapper. First `this: &Name` marks instance method; no `this` marks static call.
- Source documentation lists primitives, strings, options, and opaque references. Inspect parser before adding another shape.

## Generated behavior

- Kotlin wrapper owns JNI `GlobalRef`, obtains Activity through `manganis::android::with_activity`, resolves class and method signatures, converts supported arguments, calls JNI, and converts result.
- Swift wrapper owns `objc2::rc::Retained<AnyObject>`, loads generated framework, looks up ObjC-visible class and selectors, sends messages, and converts supported values.
- `AndroidArtifactMetadata` and `SwiftPackageMetadata` are serialized into linker sections. Dioxus CLI extracts them and installs Android Gradle artifacts or Swift packages during build.
- Generated API returns `Result` or optional failures in several setup paths. Preserve context at app boundary instead of panicking.

## ABI design

- Keep foreign surface small and versioned. Prefer strings carrying validated JSON for complex DTOs until macro supports exact typed shape and errors you need.
- Add explicit schema version to JSON. Parse once on both sides. Return typed error envelope rather than magic empty string or null.
- Large binary payloads need file, buffer, or stream design outside basic string bridge. Avoid base64 unless size is bounded and measured.
- Swift and Kotlin names, package namespace, ObjC exposure, selectors, nullability, and integer widths must match generated assumptions exactly.
- Unsigned Rust integers can cross signed foreign ABIs through generated casts. Verify range before call and use signed or string wire type when full unsigned range matters.

## Threading and lifecycle

- Generated Kotlin calls attach current native thread to JVM. Android UI APIs still require main looper; dispatch inside Kotlin adapter when needed.
- Swift UIKit APIs require main thread. Dispatch in Swift adapter or enforce caller context.
- Generated Swift opaque wrappers are marked `Send + Sync` at this pin. Do not infer native object thread safety from that marker; constrain UI objects to main thread.
- Opaque wrappers own native references, but native callbacks back into Rust are not established by basic declarations. Build explicit callback registry only after proving ownership, thread, removal, and unwind behavior.
- Never unwind Rust panic through JNI or ObjC. Convert panic-prone app logic before boundary and return error.
- Release listeners, delegates, sensors, and Activity or view references through explicit close method. Rust wrapper drop alone cannot infer every foreign subscription.

## Packaging and permissions

- Native source packaging does not grant platform permissions. Declare Android manifest and Apple plist or entitlement requirements through current Dioxus app configuration, then request runtime permission where OS requires it.
- Keep secret material out of packaged Swift, Kotlin, JS, and assets. Client binaries are inspectable.
- Test real device or emulator release build. Host compilation cannot prove Gradle, SwiftPM, linker, runtime permission, class lookup, or selector behavior.

## Known macro limits at pin

- Public docs are sparse and source implementation is alpha-era Manganis code.
- Supported type documentation and parser behavior may diverge at edges. Inspect `packages/manganis/manganis-macro/src/ffi.rs` for exact requested signature.
- Async foreign functions, callbacks, collections, arbitrary structs, overloads, generics, and rich exception mapping aren't documented as supported public surface.
- Activity cache refresh after recreation has no public path in pinned `with_activity`.
- Unsigned JNI arguments are cast to signed JNI primitives without range checks. Swift strings with embedded NUL can panic during generated conversion. Validate at Rust boundary.
- Static Swift method generation, named Swift argument labels, and non-string object return conversion have unresolved source-level limitations. Prove exact generated code before claiming support.
