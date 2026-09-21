---
date: 2026-09-21
topic: android-native-feel
focus: make the SHPRD Android shell feel native (adaptive icons, splash, themed bars, back gesture, WebView polish, low-cost native affordances)
---

# Ideation: Android-native-feel for the SHPRD shell

## Codebase Context

- The Android app is **not** a Kotlin project in-tree: `crates/shprd-shell` is a Rust/Dioxus 0.7.10 crate; `dx build` **generates** the Android project into `target/dx/shprd-shell/release/android/app`, and `scripts/build-android.sh` then copies `server/public` assets in and runs `gradlew :app:packageRelease`. There is **no committed `res/` tree**, and `Dioxus.toml` (identifier `dev.nebuless.shprd`) has no icon/splash/theme fields. Any native customization must be an **idempotent post-generation injection** in `build-android.sh` (dx regenerates `target/` on every build).
- Android flow today: `MOBILE_BOOTSTRAP` (in `crates/shprd-shell/src/main.rs`) reads `localStorage["shprd-host-url"]`; if present it `window.location.assign(host)` — a **full-page navigation away** from the local shell into the remote bridge. Back gesture, downloads, file chooser, share, and bar theming are therefore whatever the wry WebView defaults allow.
- Web side already has `viewport-fit=cover`, a pre-paint theme script (persisted theme → `data-theme` + `color-scheme`), and pinch-zoom suppression; it ships a **single** `web/public/shprd-icon.png` and **no `theme-color` meta tag**.
- 2026 external facts (researched): adaptive icons require `mipmap-anydpi-v26` XML (108dp layers, 66dp safe zone, per-density PNGs 108–432px, flat alpha-only monochrome for Android 13 themed icons, 512×512 Play listing); splash on API 31+ = `androidx.core:core-splashscreen` (`Theme.SplashScreen`, `windowSplashScreenBackground/AnimatedIcon`, `postSplashScreenTheme`); targetSdk 35 **forces edge-to-edge** and deprecates `statusBarColor`; Chromium WebView forwards `safe-area-inset-*` (M136+) and `ime()` visual-viewport resize (M139+); `navigator.share` is **not implemented** in WebView; TWA is disqualified for SHPRD (needs remote PWA + assetlinks; cannot host the connect form or injected bridge).
- Comparables: **Home Assistant Companion** (persistent WebView + overlay state machine + JS-bridge V2 — the direct analog, but it owns Kotlin); **Immich** (dark/light splash pairs are table stakes); **Jellyfin** (minimal wrapper that inherits the web theme — the gap SHPRD risks); **Tailscale** (fully native feel bar); **OpenChamber** (reference app: PWA icon ladder any+maskable 192/512, `theme_color`/`background_color`, pre-paint `splashBg` keys to kill the cold-launch white flash).
- Terminal constraint: nothing may silently kill an active terminal session (back-exit, pull-to-refresh, refresh).

## Ranked Ideas

### 1. Idempotent native-overlay injector in `build-android.sh`
**Description:** One deterministic pass between `dx build` and `gradlew :app:packageRelease` that applies every native surface — `res/` tree (icons, splash theme, values/values-night themes), `AndroidManifest.xml` overlay (theme attach, `enableOnBackInvokedCallback`, future intent-filters) — into the generated project, guarded so re-runs and dx upgrades never duplicate or break.
**Rationale:** dx wipes `target/` on every build, so hand-placed res files are non-idempotent; the injector is the single lever that makes every other cheap native affordance possible without Kotlin, and it turns native feel into a reproducible build artifact.
**Downsides:** Must survive gradle regen and dx version bumps; needs a small test (`scripts/android-package.test.ts` exists to extend).
**Confidence:** 92%
**Complexity:** Medium
**Status:** Unexplored

### 2. Adaptive + monochrome launcher icon from one 1024×1024 source
**Description:** Commit one 1024px brand PNG; generate the full `res/` tree (`mipmap-anydpi-v26/ic_launcher.xml` with background `#1a1b26`, foreground, flat-alpha monochrome; per-density PNGs; 512px Play listing) once with a generator (e.g. `@capacitor/assets` standalone or Image Asset Studio) and inject via the injector (idea 1). Monochrome layer gives Android 13 themed icons.
**Rationale:** The launcher icon is the first native-feel tell, felt daily by every user; today the single un-maskable `shprd-icon.png` crops unpredictably and themed-icon users see an untinted outlier.
**Downsides:** Safe-zone cropping needs one human eyeball pass; generator output needs review into `assets/` (committed) rather than build-time npm dependency.
**Confidence:** 90%
**Complexity:** Low-Medium
**Status:** Unexplored

### 3. Dark-mode-aware branded splash (API 31+ compat)
**Description:** `core-splashscreen` theme: `windowSplashScreenBackground` = `#1a1b26` (dark) with a values-night light pair, brand icon layer, `postSplashScreenTheme` for the main theme. Splash is **Activity-level**, so it survives the full-page WebView navigation to the remote host — the only fix for the cold-launch white flash that does not depend on where the WebView later navigates.
**Rationale:** A dark terminal app flashing white/blank on every cold launch reads as broken; Immich/OpenChamber/HA all treat brand-colored, dark-mode-aware splash as table stakes.
**Downsides:** Theme must actually attach to the dx-generated Activity (parent-chain conflict is the [VERIFY] risk); compat library adds a dependency to the generated gradle build.
**Confidence:** 88%
**Complexity:** Low-Medium
**Status:** Unexplored

### 4. Theme-color + edge-to-edge system-bar theming
**Description:** Add `<meta name="theme-color">` (dark `#1a1b26` / light pair) to `web/index.html`; verify whether wry/Chromium applies it to bar appearance after the remote navigation, and set `values-night` `windowLightStatusBar`/`windowLightNavigationBar` toggles in the injected theme so bar icon appearance always matches; no deprecated `setStatusBarColor`, no fighting targetSdk-35 edge-to-edge.
**Rationale:** Status/nav bars that don't match `#1a1b26` are the "browser letterbox" tell; the web-side meta gap is the blocker today and is one line.
**Downsides:** wry's own inset handling may override or double-pad; needs a device check ([VERIFY] which layer wins on the Chromium rev shipped by current WebViews).
**Confidence:** 75%
**Complexity:** Medium
**Status:** Unexplored

### 5. Keyboard/safe-area/terminal-correctness hardening (config + verification)
**Description:** Verify on-device that `env(safe-area-inset-*)` (M136+) and `ime()` visual-viewport resize (M139+) reach the remote page with `viewport-fit=cover` already present; pin WebView config where reachable (`textZoom=100` for the terminal grid); explicitly **refuse** algorithmic darkening (would corrupt terminal colors) and mixed content; document the device-matrix check as a release gate.
**Rationale:** A terminal lives on typing — keyboard covering the bottom lines is the daily-use tell; this is mostly verification plus config, not a feature.
**Downsides:** Behavior varies by WebView/Chromium rev; some settings may not be reachable from wry and fall to the generated-project layer ([VERIFY]).
**Confidence:** 80%
**Complexity:** Low
**Status:** Unexplored

### 6. Persistent local shell around remote content (go/no-go architectural fork)
**Description:** Replace the full-page `window.location.assign(host)` with a mounted shell (HA-style Loading/Content/Error overlay state machine) that hosts remote content, keeping local control of back, theming, downloads, file picker, share, shortcuts, and notifications. This is not a task in this track — it is the **decision gate**: every funnelled idea below (back-with-memory, file picker, DownloadManager, share passthrough, shortcuts, notifications, keep-screen-on, file opener) is only feasible if the shell owns a native WebView/Activity surface, which means Kotlin or a wry fork — a multi-week-to-month commitment HA paid by being a purpose-built native app.
**Rationale:** It is the only path to the deep native affordances (D/F/G/H/J/K/L/M from the candidate pool), and writing them against wry's unexposed hooks is impossible today.
**Downsides:** Real cost, currently unbounded; wry 0.7 feasibility unproven ([VERIFY]); risks destabilizing a working shell.
**Confidence:** 60%
**Complexity:** High
**Status:** Explored (deferred decision — explicit go/no-go, not in low-cost scope)

## Cross-cutting synthesis

Ideas 1+2+3(+O's icon ladder for the web track) compound into one deliverable: **"one brand source → launcher + splash + web icons through one idempotent build step"** — a single engineering asset paying off on launcher, cold launch, recents, Play listing, and browser/PWA surfaces simultaneously. Ideas 4+5 are the "OS chrome matches the terminal" pair and share the same verification pass.

## Rejection Summary

| # | Idea | Reason Rejected |
|---|------|-----------------|
| R1 | PWA manifest as shell's icon/theme source (O) | wry shell has no PWA installability path; manifest only serves the browser — duplicates the icon ladder; web-track's job |
| R2 | Back-with-memory routing (D) | Requires OnBackPressedDispatcher in the generated Activity — wry owns system back with no hook; funnels into idea 6 |
| R3 | File picker handoff (F) | `onShowFileChooser` is a WebChromeClient callback wry does not expose; HA ships it only because it owns Kotlin; funnels into idea 6 |
| R4 | DownloadManager for artifacts (G) | `DownloadListener` hook not exposed by wry; backgrounding also touches never-kill-session; funnels into idea 6 |
| R5 | Share-sheet passthrough (H) | Needs JS-bridge + Java Context for ACTION_SEND — impossible on wry today; funnels into idea 6 |
| R6 | `shprd://` deep links (I) | Manifest intent-filter is cheap (kept as injector capability), but intent routing into the WebView needs Activity access; niche value until idea 6 |
| R7 | Per-host home-screen shortcuts (J) | ShortcutManager needs Activity/Context + bridge; high native cost, low CLI-tool value; funnels into idea 6 |
| R8 | Run-completion notifications (K) | POST_NOTIFICATIONS + foreground-service lifecycle is architectural on a thin shell; session-survivability risk; funnels into idea 6 |
| R9 | Keep-screen-on during sessions (L) | Runtime Window flag needs Activity access wry does not expose; top priority *inside* idea 6 if it goes ahead |
| R10 | External file opener (M) | FileProvider res is cheap but ACTION_VIEW needs a Context; only matters after R4; funnels into idea 6 |
| R11 | Pull-to-refresh on connect screen | Session-killer risk pattern on a terminal app; connect screen is one tap from done — low value vs risk of normalizing PTR |

## Session Log
- 2026-09-21: Initial ideation — 32 candidates generated (4 frame agents via 9router cheap models), 16 after dedupe/merge, 5 low-cost survivors + 1 deferred go/no-go fork (N) survived adversarial critique; 11 rejected with reasons.
