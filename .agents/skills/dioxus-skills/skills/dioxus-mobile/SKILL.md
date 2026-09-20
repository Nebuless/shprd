---
name: dioxus-mobile
description: Builds and diagnoses Dioxus Android and iOS apps. Use when working on mobile launch, lifecycle, permissions, native Kotlin or Swift plugins, assets, dx packaging, WebView behavior, or mobile graphics questions.
metadata:
  invocation: model
---

# Dioxus mobile

1. Verify `/root/repo/dioxus` is at `fda3dc9c2b10ddf4417edcbb98caa9613ac92d26`. Treat that checkout as authority. Label guidance from [official mobile docs](https://dioxuslabs.com/learn/0.7/guides/platforms/mobile) as **Docs explanation** and any conflict or unsupported detail as **Docs mismatch**.
2. Model mobile as target-specific paths inside `dioxus-desktop` and `dx`. No standalone mobile crate exists at this SHA. Read [`references/architecture.md`](references/architecture.md) before changing launch, lifecycle, WebView, Tao, Wry, or WGPU behavior.
3. Pick platform boundary. Read [`references/platform-integration.md`](references/platform-integration.md) for permissions, Android JNI and Kotlin, Swift plugins, or iOS host questions. Preserve every iOS host unknown as unknown until local source proves it.
4. Read [`references/build-and-assets.md`](references/build-and-assets.md) before changing `Dioxus.toml`, assets, Gradle, Xcode tools, signing, APK, AAB, app, or IPA output.
5. Read [`references/debugging.md`](references/debugging.md) for startup crashes, lifecycle recreation, blank WebViews, emulator failures, native plugin failures, or graphics confusion.
6. Verify through the target surface. Android work needs emulator or device launch plus `adb logcat`; iOS work needs simulator or device launch plus Apple tooling logs. Inspect generated bundle layout when the issue concerns assets, manifests, native libraries, frameworks, or signing.

Finish only when platform, target triple, generated host boundary, runtime permission path, packaged artifacts, and observed device behavior agree. If local source does not expose the required iOS host contract, report the gap instead of inventing Swift glue.
