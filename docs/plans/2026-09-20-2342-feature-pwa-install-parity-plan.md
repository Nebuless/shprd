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

## Planning Contract

### Key Technical Decisions

- **KTD1. Host grammar is generated once, consumed four ways.** A build-time generator (`scripts/gen-host-rules.ts`, run in `build:web` before Vite and referenced by `cargo` builds via a checked-in generated JSON) emits (a) the canonical predicate rule set and (b) a golden-vector fixture (valid origins; rejects credentials/query/fragment; path-only inputs auto-correctable). Rust `HostUrl::parse` consumes the JSON via `include_str!` + `#[test]` over the same vectors; `remoteHost.ts` imports a generated TS module; `MOBILE_BOOTSTRAP` inlines the generated JS predicate (same string-injection mechanism `mobile_index()` already uses — its split-on-module-script injection preserves script order); `shell-controls.js` imports/inline-includes the same module at build. Query strings remain hard-rejected per the Product Contract's Key Decision (secret-refusal stance); path-only inputs auto-correct with visible feedback. Governs R4.
- **KTD2. Compression: runtime, not baked siblings.** `responseHeaders()`/`serveStatic` gain `cache-control: public, max-age=31536000, immutable` + ETag on hash-named assets; compression is performed at request time by Bun's gzip when the client advertises it. Rejected alternative: baked `.br`/`.gz` siblings — `gen-public.ts` walks `server/public` wholesale, so every sibling would be base64-embedded into the single-file binary, inflating the shipped artifact (the exact tension the Product Contract flags); runtime compression costs per-request CPU on a LAN server, which is the right trade here. The budget script gains a line asserting the measured embedded-binary delta stays within the 12 MiB envelope. Governs R11.
- **KTD3. Service worker is a build-generated, classic-IIFE, iOS-safe file.** A small Vite plugin (in-repo, in `web/vite.config.ts`) emits `sw.js` + a precache manifest from the build's `.vite/manifest.json` (the same walk `check-web-assets.mjs` does) after the build; the SW is a hand-authored classic script (no importScripts of build chunks) — OpenChamber's iOS-safe precedent — registering only when `navigator.serviceWorker` exists and the page is served over http(s). Precache = hash-named assets + entry shell; index.html stays network-first honoring the server's revalidation contract; API/WS are never intercepted. Governs R8, R9, R10.
- **KTD4. Update UX replaces the poll machine.** "Restart to apply" rides the existing store `Notice` toast (`App.tsx` ToastMark); `reloadWhenUpdatedServerIsReady`, `lazyWithReload`'s crash-reload, and the ConfigMenu/TerminalView reload call sites converge on one SW-lifecycle owner (controllerchange → single navigation). The health-poll machine is deleted in the same change that ships the SW, not after. Governs R9.
- **KTD5. Font subsetting is census-driven and budget-gated.** A build step (`scripts/subset-symbols.mjs` + `pyftsubset` as a devDependency tool) scans the web client source for used Private-Use-Area codepoints, subsets `herdr-nerd-symbols.woff2` to the census (threshold: all glyphs found in source; reviewer-visible diff in the budget script), runs BEFORE the Vite build (per gen-public wholesale-walk constraint), and `check-web-assets.mjs` gains a font-bytes budget so the 902 KB file can never silently return. The declared `unicode-range` stays unless the census proves a narrower live set. Governs R12.
- **KTD6. Probe taxonomy rides the existing `/api/health` shape.** The client-side connectivity module maps the confirmed `{ok, version, socket}` + 401 + network-failure classes onto states (reachable-ok / needs-auth / version-stale / unreachable / device-offline); no server wire change is required in this track beyond returning the fields it already returns. The Android bootstrap's preflight stays reachability-only (opaque no-cors fetch), per the trust-boundary split in the Product Contract. Governs R7.
- **KTD7. QR pairing uses the `qrcode` package on the desktop side** (OpenChamber precedent: `QRCode.toDataURL`), rendering the validated bare origin; the Android form gains a paste/scan input path that funnels into the same grammar + preflight (F1). No token endpoint in this track. Governs R13.

### Assumptions

- The bridge is served over http(s) with valid TLS for installs (Tailscale serve or equivalent); insecure origins will not prompt or register SWs (secure-context requirement).
- `/api/health` keeps its `{ok, version, socket}` shape (or grows compatibly).
- The Dioxus shell keeps injecting its bootstrap into the built `index.html`; head-tag and manifest additions must survive the `mobile_index()` string split.

### Sequencing

U1 → U2 → U3 → U7 → U4 → U5 → U6. U1 unblocks U2 (form consumes the generated grammar) and U6 (QR carries the validated origin). U3's serving changes precede U4 so the SW precache is byte-consistent from day one; U7 precedes U4 so the precache locks in the smaller font. U5 depends on U3 (probe taxonomy) and U4 (cached shell for offline render); U6 depends on U1 + U5's switcher surface.

### High-Level Technical Design

**Connect contract (component flow):**

```
[QR / typed / pasted input]
        │  (one grammar, golden vectors)
        ▼
[generated host-rules] ──► Rust HostUrl::parse      (include_str! JSON + #[test])
        │                 ──► MOBILE_BOOTSTRAP JS   (inline generated predicate)
        │                 ──► remoteHost.ts          (generated TS module)
        │                 ──► shell-controls.js      (same rule set)
        ▼
[reachability preflight] ── ok ──► location.assign ──► first-party probe (/api/health taxonomy)
        │ fail                                                │
        ▼                                                     ▼
[#shprd-mobile-error classes + edit/retry]          [connection banner states]
```

**Update lifecycle (state machine):**

```
[running vN] ── server update ──► [staging vN+1 (SW background)]
      ▲                                    │ revision ready
      │                                    ▼
   [reload] ◄── controllerchange ◄── [Restart to apply? ── accept]
      │            (single owner; health-poll machine retired)
      └──────────────── next cycle
```

**Serving decision (compression + cache):**

```
request /assets/* ──► responseHeaders(): immutable + ETag ──► runtime gzip if advertised
request /index.html ─► no-cache, must-revalidate (unchanged atomic commit point)
request /manifest.webmanifest ─► generated, no-store (before auth gate)
```

## Implementation Units

### U1. One host grammar: generated validator + golden vectors across all four surfaces

Implements R4. Cites Product Contract Key Decision "Query strings stay hard-rejected" (governs R4) and the connect-contract trust-boundary decision.

**Scope:** Add `scripts/gen-host-rules.ts` emitting `server/src/generated/host-rules.json` (checked in) + `web/src/generated/host-rules.ts`; rewrite `HostUrl::parse` to consume the fixture with `#[test]` coverage over the vectors; `remoteHost.ts` consumes the generated module (keeping its public API); `MOBILE_BOOTSTRAP`'s `normalize` is replaced by the generated predicate + path auto-correction with visible "path removed" feedback; `shell-controls.js` consumes the same rule set at build. Rejection copy unchanged for query/credentials ("Enter an HTTP(S) origin without credentials, path, query or fragment" stays the secret-refusal contract); path inputs auto-correct per AE1.

**Test scenarios:** happy path — each of the four surfaces accepts the same valid-origin vectors (https host, IP+port, Tailscale machine name) and derives the same WS origin; error path — credentials/query/fragment inputs are rejected identically in all four; edge case — trailing-slash and path-only inputs auto-correct to the origin with feedback; integration — `remoteHost.test.ts` gains the golden vectors; Rust `#[test]` runs the same fixture.

### U2. Android connect form: preflight, classed errors, saved-host gate

Implements R5, R6; depends on U1. Cites Product Contract connect-contract Key Decisions (trust-boundary split; saved-host never auto-navigates past a failed check).

**Scope:** In `MOBILE_BOOTSTRAP`: bound (timeout) no-cors preflight of `{origin}/api/health` before `window.location.assign`; failure classes reachable-unreachable/timeout/bad-scheme rendered into `#shprd-mobile-error` with per-class copy and Edit host / Retry actions; saved-host auto-start gated on preflight pass with form fallback (host prefilled, failure surfaced); persist to `localStorage["shprd-host-url"]` only after a pass.

**Test scenarios:** happy path — reachable host navigates after preflight pass; error path — unreachable host shows the classed alert, no navigation, host not persisted; edge case — saved host with powered-off server lands on the prefilled form (AE3), retry recovers when the server returns; integration — browser.mjs fixture drives the form with a stubbed fetch.

### U3. Server: probe taxonomy, serving contract, manifest route, icon set, head tags

Implements R1, R2, R7, R11; depends on nothing (slots into the existing if-chain before the auth gate, next to `/health`).

**Scope:** `server/src/index.ts`: add unauthenticated `/manifest.webmanifest` route (server-generated, `cache-control: no-store`, name/short_name/id/scope/start_url/display/theme_color/background_color/icons incl. maskable 192+512; OpenChamber-parity overrides hook left for per-host naming). `static-files.ts`: immutable cache-control + ETag for hash-named assets; runtime gzip negotiation; index.html contract untouched. Build: derive icon variants (192/512/maskable/apple-180) from one source via the existing icon pipeline in `gen-public.ts`'s walk; emit head tags (`theme-color`, `apple-mobile-web-app-*`) + `<link rel="manifest">` into the built `index.html` (Vite transform or a small postbuild step) — Dioxus `mobile_index()` split preserves them. Client: map `/api/health` + 401 + network failures onto the R7 taxonomy in a small connectivity module consumed by the update poll and banner.

**Test scenarios:** happy path — manifest served no-store with all required fields; hashed asset second request carries immutable + 304 via ETag; error path — 401 maps to needs-auth; edge case — manifest stays valid when server version changes (no stale name); integration — `static-paths.test.ts` gains header assertions; a manifest-fields test validates against the PWA required-members list.

### U7. Font subsetting + budget-gate extension

Implements R12; depends on nothing; must precede U4 (precache locks in byte costs).

**Scope:** Add the census+subset build step (source-scanned PUA codepoints → `pyftsubset`; threshold reviewed, not script-defaulted); keep the declared `unicode-range`; wire into `build:web` before Vite; extend `check-web-assets.mjs` with a font budget so the 902 KB file cannot silently return; record the measured before/after in the PR description.

**Test scenarios:** happy path — build produces a subset font that renders the same terminal glyphs in the browser fixture (visual diff on the sample glyph set); error path — census tool fails loudly when the font file is missing; edge case — budget gate fails the build if a future asset pushes fonts over the new cap; integration — postbuild budget check passes with the subset in place.

### U4. Service worker update pipeline

Implements R8, R9, R10; depends on U3 (serving contract), U7 (final byte costs).

**Scope:** Vite plugin emitting `sw.js` (classic IIFE) + precache manifest from `.vite/manifest.json`; registration in the web entry (secure-context gated, skipped on non-http(s)); cache-first hashed assets, network-first index.html; message-driven skipWaiting; staged-download → "Restart to apply" Notice toast → controllerchange → single navigation; delete `reloadWhenUpdatedServerIsReady`, `lazyWithReload`'s reload, and the ConfigMenu/TerminalView reload call sites (converge on one owner); the SW's fetch handler never touches `/api/*` or the WS.

**Test scenarios:** happy path — staged update: SW detects a new precache revision, toast appears once, accept lands on the new version, no double reload; error path — server down at launch renders the cached shell with the unreachable state (AE7); edge case — stale tab lazy-chunk 404 is absorbed by the precache hit (no crash reload); integration — budget gate asserts precache size ≤ the existing envelopes (12 MiB / 650 KiB / 200 KiB gzip).

### U5. Web client: install CTA, iOS coach card, standalone switcher, connectivity states

Implements R3, R7, R15; depends on U3 (taxonomy + manifest) and U4 (cached shell).

**Scope:** New `useInstallPrompt` hook (capture + defer + persisted dismissal, suppressed until first successful connect, hidden in standalone via `isStandaloneDisplay`); iOS A2HS coach card in the same slot; standalone host switcher — publish the active origin to the page (bridge-injected `__SHPRD_HOST_URL__` when served by the bridge, else read localStorage), port ConfigMenu's switch/confirm/test UI out of its `parent !== window` gating; connectivity banner driven by U3's taxonomy with distinct copy per state (text-not-color-only per DESIGN.md).

**Test scenarios:** happy path — post-connect CTA shows once in a non-standalone tab and stays dismissed after dismissal (AE5); error path — unreachable bridge in an installed PWA shows the cached shell + classed banner (AE7); edge case — the switcher works when `shellHostUrl()` is absent (standalone) and still works inside the desktop iframe; integration — ConfigMenu tests cover both surfaces.

### U6. QR pairing (desktop QR + mobile scan/paste)

Implements R13; depends on U1 (validated origin) and U5 (switcher surface). Cites KTD7; the Product Contract's pairing-token deferral (R14) is respected — v1 carries the bare origin only.

**Scope:** Desktop: "Pair device" affordance in the ConfigMenu host section rendering a QR (qrcode dep, `toDataURL`) + copy-link of the validated origin. Android: "Paste link" path in the bootstrap form accepting an origin (optionally with a query — the generated grammar accepts-then-strips the pair parameter before validation, coordinated with U1's vectors); clipboard read via existing `navigator.clipboard` patterns.

**Test scenarios:** happy path — desktop renders a QR encoding exactly the validated origin (inspectable string, no credentials/query) (AE9); error path — mobile paste of a host+path input auto-corrects and validates per AE1; edge case — clipboard permission denied falls back to manual typing; integration — golden vectors cover accept-then-strip.

## Verification Contract

- `bun test` at repo root and `cd web && bun test` — all existing suites green; new suites: host-rules golden vectors (TS), static-files header/manifest tests, connectivity taxonomy tests, install-prompt/switcher component tests.
- `cargo test -p shprd-shell` — Rust `#[test]` block over the golden vectors + `HostUrl::parse` regressions.
- `node crates/shprd-shell/tests/browser.mjs` — shell bootstrap flow with stubbed preflight (typed connect, saved-host recovery, pasted link).
- `bun run build:web` + `bun run check:web-assets` (budgets incl. new font gate) — must pass; record measured font/total deltas in the PR.
- Manual PWA audit on a deployed bridge: Chromium install prompt + manifest validity (AE6), iOS A2HS result, Tailscale cold-start second-visit byte comparison (AE8), update flow end-to-end (AE7).
- Lighthouse PWA category ≥ installable on the served origin.

## Definition of Done

- Global: all Verification Contract commands green on `feat/pwa-install-parity`; no console errors in the browser fixture; budget gates pass; Product Contract AE1-AE9 each demonstrated (automated where the suite covers it, recorded manual audit otherwise).
- Per unit: the unit's test scenarios pass; the unit's cited R-IDs hold; no behavior beyond the unit's scope leaks into shared files without a plan note.
- The named Android bug is closed per AE2/AE3 (loud, classed, recoverable without clearing site data).
- The health-poll reload machine is fully retired (no surviving callers) once U4 lands.

## Appendix

### Research anchors (file:line, from planning research)
- Bun.serve route chain: `server/src/index.ts:1180` (`/health`, `/api/login`, auth gate, `/ws`, `/api/health` JSON `{ok,version,socket}` at :1223-1225, `serveStatic` fallback).
- Serving headers: `server/src/http/static-files.ts:61-70` (index.html `no-cache, must-revalidate`; others content-type only); `responseHeaders()` is the single edit surface.
- Build: root `package.json` `build:web` → `web` build + `postbuild` budget check; `scripts/gen-public.ts` embeds every `server/public` file into `server/src/public-files.gen.ts` (`.br` siblings would be embedded — the KTD2 rationale).
- Dioxus injection: `crates/shprd-shell/src/main.rs` `mobile_index()` `include_str!` + split-on-module-script injection; `MOBILE_BOOTSTRAP` const with `normalize()`, `#shprd-mobile-error`, `shprd-host-url` storage key (:38), saved-host auto-start (:55-56).
- Reload sites to retire: `web/src/store.ts:650` (`reloadWhenUpdatedServerIsReady`), `web/src/lazyWithReload.ts:19`, `web/src/components/ConfigMenu.tsx:70`/`:534`, `web/src/components/TerminalView.tsx:2277`.
- Web UI anchors: store `Notice` + `App.tsx` ToastMark (:225); ConfigMenu host section (:564-626, `shellHostUrl()` gate); `shellBridge.ts` `parent === window` gate; `downloadFile.ts` `isStandaloneDisplay` (:45, :84-91); clipboard precedent `AgentHistoryCard.tsx:68`.
- Font: `web/src/styles.css:285-290` single `@font-face`, `unicode-range U+E000-F8FF, U+F0000-FFFFD, U+100000-10FFFD`, file 902,836 B.
- Tests: bun test colocated (`server/src/http/static-paths.test.ts`, `web/src/remoteHost.test.ts`); Rust in-file `#[test]` (`lib.rs:98` block) + `crates/shprd-shell/tests/browser.mjs`.
