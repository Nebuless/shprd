---
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-brainstorm
execution: code
date: 2026-09-21
topic: feat-pwa-install-parity
focus: first-class PWA installability, offline shell, load performance, install UX + Android host-connect bug fix
mode: repo-grounded
---

# PWA Install Parity — Requirements-Only Unified Plan

## Goal Capsule

**Objective:** A user can install SHPRD's web client from their own bridge (over LAN or Tailscale HTTPS) as a first-class app — correct name, icon, and display on the home screen; updates that arrive as a single "Restart to apply" moment; a shell that loads and renders honestly when the bridge is unreachable — and, on Android, entering or pairing a host server URL connects reliably: an unreachable or mistyped host produces a loud, actionable error and a recoverable app, never a silent blank navigation loop.

**Product authority:** SHPRD worktree `feat-pwa-install-parity`; product framing decisions trace to the committed ideation artifact `docs/ideation/2026-09-21-feat-pwa-install-parity-ideation.html` (8 ranked survivors, verifier-checked) and the user's confirmed synthesis (2026-09-21).

**Open blockers:** none.

**Means (from ideation, enriched by ce-plan):** server-owned install identity + service-worker update pipeline + serve-what-you-verify performance + a rebuilt connect contract (validator, preflight, probe taxonomy, standalone switcher, QR pairing).

## Product Contract

### Summary

Make the served web client a first-class installable PWA — generated manifest and icon set owned by the bridge, an install CTA earned after first successful connect, a service-worker update pipeline with a staged "Restart to apply", immutable+compressed asset serving, and a leaner symbol font — and fix the Android host-connect bug by replacing blind `location.assign` with a connect contract: one validated host grammar, a reachability preflight with an edit-host escape, a first-party probe taxonomy, an in-app host switcher outside the desktop iframe, and QR pairing.

### Problem Frame

Today the client has zero install surface: `web/index.html` links only an icon and apple-touch-icon (one 512px PNG), and there is no manifest, theme-color, or install-prompt handling anywhere repo-wide — even though `FEATURES.md:309-310` claims an installable PWA. The build measures a 200 KiB initial-JS gzip budget and a 12 MiB total budget, but the server serves hashed assets with content-type only: no compression, no ETag, no cache-control (except index.html's `no-cache, must-revalidate` update contract), so every cold start over Tailscale re-downloads megabytes; a single 902 KB symbol font dwarfs the initial-JS budget. Updates are discovered the hard way: an open tab's lazy chunk 404s after the server swaps builds and crashes into a full-page reload (`lazyWithReload.ts`), while the client separately polls `/api/health` to reload. On Android, "connect" is an unconditional `window.location.assign(host)` (`crates/shprd-shell/src/main.rs:53`) with no reachability check; a saved bad host auto-navigates on every launch before the form can render, the same host predicate is hand-copied in four places, and the switch/confirm/test UI exists only inside the desktop iframe — in a browser tab or installed PWA it is dead code.

### Key Decisions

- **Server-generated, no-store `/manifest.webmanifest`** (user-directed, chosen over a static checked-in manifest: OpenChamber parity, per-host naming, dynamic shortcuts; static-file fallback noted for ce-plan's consideration). Governs R1, R4.
- **Offline = app shell + honest disconnect states, not offline data** (user-approved: the workspace is live WS data; the SW caches the shell, never agent/session data). Governs R6, R8.
- **Query strings stay hard-rejected in the host grammar** (session-settled: user-approved — the validator's secret-refusal stance, `lib.rs:14` "never includes the potentially secret input value", is preserved; auto-correct strips paths only; QR pairing v1 carries the bare origin). Governs R3, R5.
- **The SW lands together with retiring the health-poll reload machine** (user-approved: avoids two parallel update systems). Governs R6, R7.
- **Connect contract split by trust boundary:** the Android bootstrap's preflight is reachability-only (opaque no-cors fetch); the richer taxonomy (wrong-service / needs-auth / ok) runs first-party via `/api/health` after navigation. Governs R2, R9.

### Requirements

**Installability surface**

- R1. The bridge serves a valid installable manifest at `/manifest.webmanifest` (name, short_name, id, scope, start_url, display standalone, theme_color/background_color from the product palette, icons 192/512 + maskable + apple-touch 180) generated per request with `cache-control: no-store`, safe for both browser installs and the Dioxus shell's string-surgered `index.html`.
- R2. The served head carries `theme-color`, `apple-mobile-web-app-capable`, and `apple-mobile-web-app-status-bar-style` so manual iOS A2HS produces a quality home-screen app; icon variants are derived from one source at build time and no 512px-only icon remains.
- R3. The desktop client offers an install entry point that captures `beforeinstallprompt`, is suppressed until the first successful bridge connect, honors persisted dismissal, never shows in standalone display mode, and shows an iOS Add-to-Home-Screen coach card in the same slot.

**Connect contract (mobile bug)**

- R4. Exactly one host-validity grammar exists (http/https origin, no credentials, no query, no fragment, no path beyond "/"), generated or golden-vector-tested so all four current copies (Rust `HostUrl::parse`, MOBILE_BOOTSTRAP `normalize`, `remoteHost.ts`, `shell-controls.js`) cannot drift; input that differs only by path or trailing slash is auto-corrected with visible feedback, and only unsalvageable input is rejected.
- R5. The Android connect form validates the host's reachability before navigating (bounded preflight: reachable / unreachable / timeout), renders failures in the existing `#shprd-mobile-error` alert with a distinct message per failure class, and persists the host to `localStorage["shprd-host-url"]` only after a passing check.
- R6. A saved host never auto-navigates past a failed reachability check at launch; the user lands on the connect form with the saved host prefilled, the failure surfaced, and edit/retry actions available (no silent boot loop).
- R7. After navigation, the first-party client classifies bridge state from `/api/health` (`{ok, version, socket}`, 401-gated) into at least: reachable-ok, needs-auth, version-stale (update pending), and unreachable, and the connection banner uses these states instead of inferring from WS timers alone.

**Update pipeline & offline shell**

- R8. A service worker precaches the app shell from the build-time asset manifest (hash-named assets cache-first; index.html network-first, preserving the server's revalidation contract), renders the cached shell when the bridge is unreachable, and never caches WS or API data.
- R9. Server updates surface as one "Restart to apply" moment: the SW stages the next version in the background, applies it via skipWaiting + controllerchange at a navigation boundary (never mid-terminal-session), and the existing hard-reload sites (health-poll reload in `store.ts`, `lazyWithReload` crash reload, `ConfigMenu`/`TerminalView` reloads) converge on this single lifecycle owner.
- R10. Precautionary budgets hold: the precache set and all new static assets stay within the existing `check-web-assets.mjs` envelopes (12 MiB total, 650 KiB initial JS, 200 KiB gzip), with font bytes added to the budget gate.

**Load performance**

- R11. Hash-named static assets are served with `cache-control: public, max-age=31536000, immutable` and an ETag, while index.html keeps `no-cache, must-revalidate`; compression (baked precompressed siblings vs runtime) is delivered for JS/CSS/SVG/font with the embedded-binary size consequence explicitly measured against the budget script.
- R12. The nerd-symbols font (902,836 bytes) is subset/sliced so the first terminal render does not download the whole font; visual glyph coverage in the terminal is preserved (subset threshold owned by the team, not a script default).

**Device pairing**

- R13. The desktop client can hand its origin to a phone via QR code and copy-link (v1 = bare validated origin; no secrets in the QR); the Android connect form accepts scan/paste input, and the handed-off URL flows through the same grammar + preflight before persisting (the handoff is the first connect test).
- R14. A pairing-token flow that pre-authes the installed PWA (redeeming a one-time token to mint `shprd_auth`) is explicitly deferred: noted as future auth work, requiring a new endpoint since `/api/login` accepts `{password}` only.

**Standalone usability**

- R15. In a plain browser tab or installed PWA (outside the desktop iframe), the active bridge origin is visible to the page and the host switch/confirm/test UI renders — "fix my host" is an in-app action, not shell-only dead code.

### How This Work Fits Together

<!-- ce-section: work-relationships -->
This plan covers the PWA track as one journey: install → connect → live app → update. The connect contract (R4-R7, R13-R15) is the foundation the install journey depends on; install identity (R1-R3) makes the installed app exist; the update pipeline + offline shell (R8-R10) and load performance (R11-R12) make the installed app good. Sub-areas were ideated and ranked together in the committed ideation artifact and are planned as one track per the mission; a later plan may split execution, citing this file.

- Connect contract — first (Depends on: nothing; Enables: standalone switcher, pairing QR, probe taxonomy reused by banners).
- Install identity (manifest/icons/head) — Enables: earned install CTA, QR pairing start_url, per-host naming.
- Serve-what-you-verify (R11-R12) — prerequisite ordering: before SW precache locks in byte costs.
- SW update pipeline — Depends on: serve-what-you-verify (byte-consistent cache), Retires: health-poll reload machine.
- Deferred (not requirements here): pairing-token auth (new endpoint), multi-host roster with warm probes, in-flight WS action queueing, push notifications, offline data sync.

### Key Flows

- F1. **First connect (Android, typed or scanned).** **Trigger:** user enters/pastes/scans a host URL in the bootstrap form. **Covers R4, R5, R13.** Grammar validates/corrects → reachability preflight → on pass: persist + navigate; on fail: classed error in the alert + edit/retry.
- F2. **Launch with saved host.** **Trigger:** app start with `shprd-host-url` present. **Covers R6.** Preflight the saved host → pass: navigate; fail: land on the form with the host prefilled, failure surfaced, edit/retry — never a blind re-navigation loop.
- F3. **Install.** **Trigger:** first successful connect in a non-standalone browser. **Covers R1, R2, R3.** Manifest + icons make the prompt available; CTA appears post-connect; dismissal persists; iOS shows the coach card.
- F4. **Update.** **Trigger:** server binary updated while a client tab/app is open. **Covers R8, R9.** SW stages the new asset set → "Restart to apply" → controllerchange at a boundary → one reload path; stale-tab lazy-chunk 404 becomes a handled safety net.
- F5. **Bridge unreachable (installed PWA).** **Trigger:** launch or reconnect while the bridge is down/offline. **Covers R7, R8.** Cached shell renders; banner shows the classified state (device offline / bridge unreachable / needs-auth) with retry + switch-host affordances.

### Acceptance Examples

- AE1. **Covers R4, R5.** Given the user types `https://host.example/app` (path present), the form shows "Connecting to `https://host.example` — path removed" and proceeds with the origin; the host is accepted, not rejected.
- AE2. **Covers R5.** Given an unreachable IP, the preflight fails within its bound and the form shows a reachable/unreachable/timeout-classed message with Edit host and Retry; no navigation occurs.
- AE3. **Covers R6.** Given a saved host whose server is powered off, app launch lands on the connect form (host prefilled, failure visible); it does not navigate to a WebView error page, and clearing site data is not required to recover.
- AE4. **Covers R4.** The four host-grammar surfaces accept exactly the same golden-vector set (valid origins; rejects credentials, query, fragment, path-only-differences handled by correction); the vector set is a build-time fixture shared by Rust and web tests.
- AE5. **Covers R3.** Given no prior successful connect, no install banner appears despite `beforeinstallprompt` firing; after one successful connect, the CTA appears once, and after dismissal it stays dismissed across sessions.
- AE6. **Covers R1, R2.** Lighthouse/manifest check passes on the served origin: installable, maskable icon present, theme-color set, and the Dioxus-packaged shell's injected `index.html` still boots the connect form unmodified.
- AE7. **Covers R8, R9.** With the SW active and the server stopped, a cold open renders the cached shell with an honest "bridge unreachable" state; after a server update, exactly one "Restart to apply" appears, and accepting it lands on the new version with no double-reload and no lost drafts.
- AE8. **Covers R11.** A second cold visit over the same network transfers only changed bytes (hashed assets served from cache as immutable; index.html revalidated).
- AE9. **Covers R13.** Scanning the desktop QR on Android prefills and validates the origin, then follows F1; the QR encodes only the origin (inspectable, no credentials/query).

### Success Criteria

- The named Android bug is closed end-to-end: a wrong host is loud, classed, and recoverable without clearing site data (AE2/AE3).
- The served client passes a PWA installability audit (manifest valid, installable, icons correct) on Chromium and produces a quality A2HS result on iOS (AE6).
- Update UX is single-path: after the SW ships, the repo has exactly one client-side reload-owner for updates, and the health-poll reload machine is gone (AE7).
- Cold-start transfer over Tailscale drops measurably (immutable + compressed assets, subset font), with budget-gate numbers recorded in the plan's verification contract by ce-plan.

### Scope Boundaries

**Deferred for later**
- Pairing-token pre-auth (`/connect?t=` redemption minting `shprd_auth`) — needs a new endpoint; v1 QR carries the bare origin.
- Multi-host roster with warm startup probes (OpenChamber desktopHostStatus parity) — after the standalone switcher proves out.
- In-flight WS action queueing while offline (drafts already retained).
- Push notifications; offline data sync; auth redesign.

**Outside this product's identity**
- Offline agent/session data or workspace editing without the bridge (live WS workspace is the product).
- Replacing the Bun bridge, the React/Vite client, or the Dioxus shell; general-purpose mobile app framework swap.

### Dependencies / Assumptions

- Assumes the bridge is reached over http(s) with valid TLS when installed (Tailscale serve or equivalent) — the preflight and install prompt both require secure contexts.
- Assumes the Dioxus shell continues to inject its bootstrap into the built `index.html`; manifest/head changes must keep that string-surgery working (AE6).
- Assumes `/api/health` keeps its `{ok, version, socket}` shape (or grows compatibly) so the probe taxonomy can be versioned without breaking the existing update poll.
- Compression choice (baked precompressed siblings vs runtime) must be resolved with the embedded-binary size measured — implementation detail for ce-plan, with the budget delta recorded.

### Outstanding Questions

- **Deferred to Planning:** manifest static-vs-generated implementation shape (decision made: server-generated; ce-plan picks route/middleware placement), compression mechanism (baked vs runtime, with binary-size measurement), SW structure (classic IIFE vs module; precache manifest emission point), golden-vector fixture mechanics (build-time generation from which source), QR library choice, exact probe-taxonomy wire format.
- **Deferred to Planning:** whether the standalone switcher reads host state from localStorage, an injected global, or both (reconciliation with the desktop iframe path).

### Sources / Research

- Ideation (committed): `docs/ideation/2026-09-21-feat-pwa-install-parity-ideation.html` — 8 ranked survivors, axes, rejection summary; verifier-checked bases.
- Brainstorm grounding dossier: `/tmp/compound-engineering-1000/ce-brainstorm/869cce4b/grounding.md` (run artifact; quotes with file:line for connect flow, serving surface, lifecycle seams).
- Claim verification: all 14 Product Contract repo-claims confirmed 2026-09-21 by a fresh-context verifier (file:line in run transcript).
- OpenChamber (reference app, clone analyzed): server-generated no-store manifest + shortcuts; minimal classic-IIFE sw.ts (push-only, iOS-safe); probe-before-connect status taxonomy; QR /connect?t= pairing; desktopHostStatus owning-run guard.
- External: PWA 2026 caching matrix (cache-first hashed, network-first HTML); Tailscale serve HTTPS model; Termux typed-host UX contrast.
