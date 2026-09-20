# Build, assets, and packaging

All facts below are pinned to Dioxus `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26` in `/root/repo/dioxus`.

## `dx` target flow

Use platform flags exposed by `dx`, such as `--android` and `--ios`, with `dx serve`, `dx build`, or `dx bundle`. Confirm exact installed CLI syntax with `dx <command> --help`; flags can drift beyond this pin.

`packages/cli/src/build/request.rs:2233-2245` verifies Rust target plus platform tooling. Android checks NDK linker at `packages/cli/src/build/android.rs:545-565`. iOS tooling verification currently returns success without checking tools at `packages/cli/src/build/apple.rs:46-83`; later Apple commands can still fail when Xcode tools, SDK, simulator, provisioning, or signing are missing.

`packages/cli/src/build/request.rs:3061-3075` starts simulator when device name is absent. Android selects first AVD and launches emulator. iOS selects a simulator through `xcrun simctl`, boots one when needed, then opens Simulator app.

## Android output

`dx` generates a Gradle application tree. `packages/cli/src/build/android.rs:81-378` writes wrapper, app module, manifest, Kotlin host, resources, and `jniLibs` directories.

- Rust binary is a shared library named `lib<lib_name>.so`, default `libmain.so`: `packages/cli/src/build/request.rs:2609-2614`.
- Per-ABI library destination is `app/src/main/jniLibs/<abi>`: `packages/cli/src/build/request.rs:2898-2905`.
- Asset destination is `app/src/main/assets`: `packages/cli/src/build/request.rs:2858-2879`.
- Debug and release APK assembly uses Gradle `assembleDebug` or `assembleRelease`: `packages/cli/src/build/android.rs:381-405`.
- AAB output uses Gradle `bundleRelease`: `packages/cli/src/build/android.rs:408-440`.
- Default SDK values are min 24, target 34, compile 34: `packages/cli/src/build/android.rs:184-186`.

Release branch selection at this SHA checks legacy `config.bundle.android` in `assemble_android`. Android signing config also exists under `[android.signing]` in `packages/cli/src/config/manifest.rs:586-590`. Inspect resolved config and Gradle task when release unexpectedly produces debug artifact. Treat this split as source behavior, not a unified promise.

## iOS output

- App root is `<name>.app`: `packages/cli/src/build/request.rs:2567-2570`.
- Executable and `assets/` sit at bundle root: `packages/cli/src/build/apple.rs:22-32` and `packages/cli/src/build/request.rs:2877-2879`.
- `Info.plist` is written during build: `packages/cli/src/build/request.rs:2021-2024`.
- Signing can auto-provision or use supplied entitlements, then invokes `codesign`: `packages/cli/src/build/apple.rs:282-360`.
- Device provisioning profile is copied as `embedded.mobileprovision`: `packages/cli/src/build/apple.rs:334-340`.
- IPA packaging requires physical-device AArch64 target and a signed `.app`: `packages/cli/src/bundler/ios.rs:27-124`.
- `dx bundle --ios` defaults to device target when none is given: `packages/cli/src/cli/bundle.rs:206-215`.

Simulator app and device IPA are distinct outputs. Do not package simulator target as IPA.

## Assets

`asset!()` metadata is embedded in binary, discovered after build, hashed, and rewritten by CLI. Evidence: `packages/cli/src/build/assets.rs:1-24` and `485-652`. Native bundle paths are platform-specific, but application references should use generated bundled paths rather than hardcoded host filesystem locations.

Android assets land in Gradle assets directory. iOS assets land in app bundle `assets/`. Wry serves app content through Dioxus custom protocol, starting at `dioxus://index.html/`, in `packages/desktop/src/webview.rs:395-430`.

When asset fails:

1. Confirm source entered extracted asset manifest.
2. Confirm bundled path after hashing.
3. Inspect final `app/src/main/assets` or `.app/assets`.
4. Confirm request URL reaches `dioxus://` custom protocol.
5. Separate missing packaged bytes from WebView cache or MIME issues.

## Package inspection

For Android, inspect generated Gradle tree, final APK or AAB contents, ABI library name, manifest permissions, and plugin modules or AARs. For iOS, inspect `.app/Info.plist`, executable, `assets/`, `Frameworks/`, `PlugIns/`, entitlements, embedded profile, and code signature.

Official [mobile guide](https://dioxuslabs.com/learn/0.7/guides/platforms/mobile) gives intended command flow. **Docs mismatch:** local source decides actual output folders, default targets, signing behavior, and generated files.
