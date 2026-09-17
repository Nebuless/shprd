# SHPRD refactor plan

## Purpose

This is the tracked continuation plan for SHPRD work on `shprd-refactor`.
It preserves durable product decisions and release gates. It does not preserve
agent sessions, test logs, generated assets, or transient `.omo` evidence.

## Product boundary

- SHPRD is a standalone agent development environment. It is not launched as a
  Herdr plugin.
- Herdr remains the control plane for workspaces, terminals, and agents.
- A Herdr plugin, if maintained, is only an integration companion.
- Retain React/TypeScript as the installed workspace UI. Rust replaces the Bun
  bridge and CLI; Dioxus remains an optional remote/Android shell that hosts
  the retained React UI rather than replacing it.
- Keep Bun bridge as default installed runtime until native host parity gates
  pass. Do not install, package, deploy, or replace the current Bun service
  with the native host before then.

## Target delivery shape

- `web/` remains the only workspace UI. Bun/Vite builds React assets during
  development and release assembly; no Bun process serves production traffic.
- `shprd` becomes the standalone Rust CLI and host. It serves packaged React
  assets, owns authentication, connection profiles, HTTP, WebSocket, terminal,
  workspace, settings, and agent transports.
- `shprd-shell` remains a Dioxus WebView launcher. Desktop and Android clients
  load React from a configured HTTPS SHPRD host; Herdr engines never run inside
  the Android application.
- The installed host command is `shprd serve --open`. It must require no Bun or
  source checkout and must locate its packaged React assets without a caller
  supplied `--public-dir`.
- The source checkout command is `shprd dev`. It builds the WIP React bundle,
  starts the Rust host, and prints the authenticated local URL. It may require
  Bun because React still needs a bundler in development.
- `shprd apk build` is a source-developer command. It invokes the pinned Dioxus
  CLI with the Android SDK, NDK, JDK, and Rust targets, then reports one APK
  path. `shprd apk fetch` is the end-user command once releases publish a
  checksum-verified APK and manifest.
- Dioxus is not a replacement for React DOM in this architecture. It provides
  native desktop and Android containers around the retained React surface.

## Delivery increments

1. **Native host package**
   - Stage or embed the output of `bun run build:web` with every `shprd`
     platform artifact.
   - Change the Rust CLI from host flags alone into explicit `serve`, `dev`,
     `desktop`, and `apk` subcommands. Preserve current host flags beneath
     `serve` and preserve environment compatibility for the user service.
   - Keep `--public-dir` as a development override only. The packaged default
     must reject missing or malformed assets before binding a public listener.
2. **React-to-Rust host parity**
   - Mount authenticated, generation-aware workspace service routes in the
     production router with real history, hook, and autosync callbacks.
   - Make retained React drive native terminal, connection, workspace, settings,
     agent, and update contracts without test-only shims.
   - Prove omo/senpi and Atomic live control, streaming, cancellation,
     reconnection, and attachment behavior. Atomic attachment to an existing
     TUI remains a release blocker.
3. **Dioxus launcher package**
   - Build a native desktop launcher and Android APK from `shprd-shell`; each
     accepts only a validated HTTP(S) host origin and keeps authentication in
     the framed host document.
   - Preserve exact-origin `shprd.shell.v1` bridge checks. Configure framing
     response headers only for explicit launcher origins.
   - Publish Android artifacts separately from the host binary. The APK is a
     remote client, not a substitute host service.
4. **Release and install**
   - Publish a Rust host binary plus React asset bundle for each supported host
     platform, a versioned APK plus checksum manifest, and install metadata.
   - Replace `shprd-studio.service` only after native-host parity gates pass;
     preserve its `HOST`, `PORT`, `SHPRD_CONFIG_DIR`, authentication, and
     rollback contracts.

## Required parity gates

- omo/senpi through its native RPC surface.
- `@bastani/atomic` through a separate adapter. Its protocol is not compatible
  with the Senpi protocol.

Both targets need live attachment, lifecycle, and user-facing parity proof.

## Current integrated work

`shprd-refactor` contains these native increments:

- Scoped connection profiles, isolated host configuration, and bounded token
  loading.
- Connection lifecycle supervision: generation-aware leases, startup/cleanup,
  retry limits, and scoped HTTP routing.
- Native terminal bridge lifecycle: protocol discovery, direct and endpoint
  transport, viewer attachment cleanup, backpressure-aware publication, and
  stale-generation fencing.
- Native workspace, settings, process, file, Git, and remote-operation modules.
  Service routes remain intentionally unmounted from the production host.
- Stale HTTP workspace deletion is guarded at mutation-lock admission.

These increments are work in progress, not a native-host release declaration.

Do not promote native host to installed/default runtime until all gates pass:

1. **Terminal and browser integration**
   - Retained React terminal controls must drive the native terminal endpoint
     without a test-only shim.
   - Verify connection identity, raw frame geometry, stale-output retirement,
     attach/detach, and backpressure through a real browser at desktop and
     mobile widths.
2. **Workspace service routing**
   - Mount only authenticated, generation-aware service routes.
   - Provide real lifecycle callbacks for history, hooks, and autosync.
   - Verify cancellation and stale safety against an actual `Manager` lease.
3. **Agent parity**
   - Prove live omo/senpi and Atomic control, streaming, cancellation,
     reconnection, and attachment behavior.
   - Atomic attachment to an existing TUI remains an explicit blocker.
4. **Dioxus and Android**
   - Keep the Dioxus shell as an optional remote/Android companion hosting the
     retained React UI; it is not a React replacement gate.
   - Verify native Android remote-client behavior on a real device before a
     Dioxus-shell release, but do not block native-host promotion on it.
5. **Release evidence**
   - Run full Rust workspace tests, strict Clippy, workspace build, retained
     frontend build, and manual browser QA after each integrated parity change.
   - Preserve Bun service and existing authentication/config isolation while
     collecting native-host evidence.

## Continuation workflow

1. Start from remote branch:

   ```sh
   git fetch origin
   git switch --track origin/shprd-refactor
   ```

2. Read [Architecture](./ARCHITECTURE.md), this plan, and applicable `AGENTS.md`
   files before editing.
3. Make one bounded parity increment at a time. Do not merge stale worker trees
   blindly; compare against current `shprd-refactor` first.
4. Keep generated output, `.omo` evidence, agent logs, and local secrets out of
   commits.
5. Commit verified increments to `shprd-refactor`; never push or merge `main`
   as part of this refactor workflow without explicit direction.

## Durable references

- Current system contracts: [Architecture](./ARCHITECTURE.md)
- Deployment and Bun runtime boundary: [Deployment](./DEPLOYMENT.md)
- Current user-facing capability guide: [Features](../FEATURES.md)
- Historical releases: [History](./HISTORY.md)
