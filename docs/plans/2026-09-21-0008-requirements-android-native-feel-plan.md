---
date: 2026-09-21
topic: android-native-feel
focus: make the SHPRD Android WebView shell feel native without a Kotlin rewrite
artifact_contract: ce-unified-plan
artifact_type: requirements
---

# Requirements: Android-native feel for the SHPRD shell

## Problem

The SHPRD Android APK is functionally a browser wrapper. Installed on a device it presents a generic single-layer launcher icon that crops unpredictably and cannot participate in Android 13+ themed icons; every cold launch flashes a blank/white canvas before the connect form or the remote bridge paints; after connecting, the status bar, navigation bar and keyboard behave as a browser's, not as part of a dark `#1a1b26` terminal product; and the back gesture can end the session the way a browser ends a page. The web frontend already ships `viewport-fit=cover`, a pre-paint theme script, and a design palette — but the Android surface around it contributes none of the native affordances users of self-hosted companion apps (Home Assistant, Immich, Tailscale) take for granted. The gap is visible in the first 60 seconds and every day after.

The structural constraint that shapes every requirement: the Android project is **generated** by `dx build` into `target/dx/shprd-shell/release/android/app` and post-processed by `scripts/build-android.sh`; nothing native can be committed in place, so all native feel must arrive through an **idempotent build-time injection** (resources, themes, manifest overlay) plus small web-side hooks. The app is a **terminal/agent tool**: no affordance may put an active session at risk.

## Goals

- G1. The installed app looks like a first-class Android app before it is ever opened: adaptive launcher icon with monochrome/themed-icon support and a correct store listing icon.
- G2. Cold launch is brand-colored and dark-mode-aware from first pixel to first paint, with no white/blank flash.
- G3. OS chrome (status bar, navigation bar, keyboard) matches the terminal theme in both light and dark modes and does not fight the terminal UI.
- G4. The build remains reproducible: every native surface is produced by committed source assets and build scripts, survives `dx` regeneration, and is CI-checkable.
- G5. The plan states explicitly which native affordances are deferred and why (wry hook limits), so the human reviewer can rule on the deferred fork with full information.

## Non-Goals

- N1. A Kotlin application rewrite or a wry/WebView fork (the "persistent local shell" architectural fork is surfaced as a decision, not implemented).
- N2. Back-gesture routing, file picker, DownloadManager, share-sheet passthrough, deep links, shortcuts, notifications, keep-screen-on: all require WebView/Activity hooks wry does not expose; they are **deferred** pending the fork decision (documented, not built).
- N3. Pull-to-refresh anywhere (terminal session-killer risk).
- N4. Mobile host-entry/connect-form URL handling — owned by the PWA-install-parity track; this plan touches it only where a native-feel change would break it.
- N5. Trusted Web Activity packaging (disqualified: requires remote PWA + assetlinks, cannot host the connect form or the shell bridge).

## Requirements

- REQ-1 **Idempotent native-overlay injection.** `scripts/build-android.sh` (or a script it calls) must, on every build, deterministically apply a committed overlay — `res/` tree, theme XMLs, `AndroidManifest.xml` additions — into the generated Android project before `gradlew :app:packageRelease`. Re-running must be a no-op-equivalent (no duplication, no drift), and the mechanism must fail loudly if the dx-generated layout changes shape. Acceptance signal: building twice produces byte-identical APK resources for the overlay; CI test asserts overlay files exist post-build.
- REQ-2 **Single-source brand icon set.** One committed 1024×1024 source PNG (derived from the existing SHPRD brand, accent `#7aa2f7` on `#1a1b26`) generates the full launcher ladder: `mipmap-anydpi-v26` adaptive XML (background + foreground + flat-alpha monochrome), per-density PNGs (108px mdpi → 432px xxxhdpi, 66/108 safe zone), round and legacy fallbacks, and a 512×512 Play listing icon. Acceptance signal: all densities present post-build; monochrome renders as a tintable silhouette; safe-zone check passes.
- REQ-3 **Brand splash, dark-mode aware (API 31+ compat).** The generated Activity uses `androidx.core:core-splashscreen`: `Theme.SplashScreen` parent, `windowSplashScreenBackground` = `#1a1b26` with a values-night light pair, the brand icon layer, and `postSplashScreenTheme`. Acceptance signal: cold launch on an API 31+ device shows brand background in both system dark and light; no white frame; pre-31 devices get a compatible theme without crash.
- REQ-4 **System-bar theming.** `web/index.html` gains `<meta name="theme-color">` pairs (dark `#1a1b26`, light value from the existing theme script); the injected theme sets light/dark status- and navigation-bar icon appearance via `values`/`values-night` (`windowLightStatusBar`, `windowLightNavigationBar`); behavior remains correct under targetSdk 35 edge-to-edge (no deprecated color calls, no double insets padding with the page's own `safe-area-inset` usage). Acceptance signal: bars visually match the page background in both themes on a device; no double padding at screen edges/keyboard.
- REQ-5 **Terminal input/output correctness preserved.** Keyboard resize behavior (`ime()` visual-viewport, Chromium M139+), safe-area forwarding (M136+), `textZoom=100`, no algorithmic darkening, no mixed-content loosening — verified on-device and documented as a release gate; any setting not reachable through wry falls to the injected theme/resources only. Acceptance signal: device-matrix checklist in the plan's verification section passes; terminal bottom line visible above keyboard.
- REQ-6 **No regression of connect flow or session safety.** The injection must not alter the bootstrap/localStorage host flow, URL validation, or introduce any UI that can reload or exit the WebView during a session. Acceptance signal: existing tests (`scripts/android-package.test.ts`, shell browser tests) pass; manual connect→terminal flow unchanged.

## Constraints

- C1. No new build-time dependency may enter the repo's Node toolchain for icon generation — generator output is committed once as reviewed assets, not regenerated per build.
- C2. The only new Android dependency allowed is `androidx.core:core-splashscreen` (and only if it can be added to the generated gradle build idempotently; otherwise the splash theme must be hand-written XML with no library).
- C3. Everything must work with the existing wry/WebView navigation model (full-page navigate to remote host). Nothing may assume native Activity code.
- C4. Docs commits follow Conventional Commits; the precommit suite (biome/eslint/tsc/bun test) must stay green.

## Success criteria

1. Installing the APK on an Android 13+ device shows a proper adaptive + themed (monochrome) icon in launcher and themed-icon preview.
2. Cold launch → splash (brand color, correct for system theme) → connect form/remote page, with no white flash at any stage.
3. Status and navigation bars match the app background in dark and light; keyboard does not obscure terminal input; no double safe-area padding.
4. `mise run shell:android` on a clean checkout produces the identical APK resources without manual steps; CI package test verifies overlay integrity.
5. The plan document records the deferred native affordances and the go/no-go fork question so a human can decide with one read.

## Open questions (for reviewer)

- Q1. Is the committed-source-asset approach for icon generation acceptable (vs. adding a per-build generator dependency)?
- Q2. Should the splash theme use `core-splashscreen` (adds a gradle dependency to the generated project) or hand-written XML only?
- Q3. The persistent-shell rearchitecture (own WebView + overlay state machine) unlocks back handling, downloads, file picker, share, notifications, shortcuts — proceed to a feasibility spike as a follow-on track, or park it?
- Q4. Light-mode bar appearance: match `values-night` pair automatically, or force dark chrome always (terminal-first product)?

## Grounding

- Ideation: `docs/ideation/2026-09-21-android-native-feel-ideation.md` (32 raw → 5 survivors + 1 deferred fork; adversarial critique recorded).
- Research synthesis: adaptive-icon/splash specs (2026), WebView native-feel checklist, HA Companion / Immich / Jellyfin / Tailscale / OpenChamber comparisons (session research, 9router dispatches; OpenChamber dist inspected locally).
