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
- Retain React/TypeScript frontend until a Rust/Dioxus replacement preserves
  feature parity and user experience.
- Keep Bun bridge as default installed runtime until native host parity gates
  pass. Do not install, package, deploy, or replace the current Bun service
  with the native host before then.

## First-class agent targets

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

## Required parity gates

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
   - Keep bounded Dioxus shell coexistence with React until replacement is
     parity-safe.
   - Verify native Android remote-client behavior on a real device.
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
