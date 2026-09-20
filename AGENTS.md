# Repository Guidelines

## Project Structure & Module Organization

This repo contains a Bun-powered bridge and a React/Vite frontend for Herdr.
Frontend code lives in `web/src`, with reusable UI under `web/src/components`,
assets under `web/src/assets`, and global styling in `web/src/styles.css`.
Server and bridge code lives in `server/src`. Release helpers live in `scripts/`.
Generated build output belongs in `web/dist`, `server/public`,
`server/src/public-files.gen.ts`, `server/shprd*`, and `dist/`; these paths
are ignored and should not be committed.

Experimental Rust components live in `crates/`, with native runtime extension
glue in `integrations/pi/`. Target shape: Rust owns CLI and host backend, React
remains workspace UI, and Dioxus provides optional desktop/Android launchers.
Keep Bun as the default until browser/API parity is verified. See
docs/ARCHITECTURE.md for current native boundaries.

## Build, Test, and Development Commands

Mise owns Rust, Cargo targets, and Dioxus CLI. Use its task entrypoints rather
than package-manager Cargo or global DX binaries.

- `mise run rust:check`: check the Rust workspace.
- `mise run shell:web`: build the Dioxus Web shell.
- `mise run shell:android`: build the Dioxus Android shell.
- `bun run dev:web`: start the Vite frontend on port 5173.
- `bun run dev:server`: start the Bun bridge with hot reload.
- `bun run build`: build frontend assets and the default standalone server binary.
- `bun run build:linux-x64`: build the Linux x86-64 standalone binary.
- `bun run build:darwin-arm64`: build the macOS Apple Silicon binary.
- `bun run package:linux-x64`: build and emit both versioned and latest `tar.xz`
  archives and checksums in `dist/`.
- `bun run package:linux-arm64`, `package:darwin-x64`,
  `package:darwin-arm64`, `package:windows-x64`, and
  `package:windows-arm64`: package the other supported release targets.
- `bun run format`: format supported files with the pinned root Biome config.
- `bun run format:check`: verify that all supported files are formatted.
- `bun run lint`: lint all TypeScript and React code.
- `bun run test`: run the Bun unit test suite.
- `bun run typecheck`: run frontend and server TypeScript checks.
- `bun run precommit`: run formatting, lint, type checks, and unit tests.
- `mise run ci`: run the reproducible local CI gate, including pinned language and hook checks.

## Coding Style & Naming Conventions

Herdr-pane subagents use the smallest capable model. Prefer Luna or Terra for
bounded tasks they can handle; reserve Astra for work whose complexity warrants
it. Verify the available model identifier and task fit before each launch.

Use TypeScript, React function components, and the existing CSS class naming
style. Commit messages must follow Conventional Commits, except automated
`Release X.Y.Z` commits. Format supported files with the root `biome.json`; do not rely on a
global or editor fallback formatter. Prefer small, focused components in
`web/src/components`. Keep manual edits ASCII unless the file already uses
non-ASCII text. Use existing store and bridge helpers before adding new
abstractions.

Root lint, format, and hook checks exclude imported `.agents/skills/` packages
and the local `crates/dioxus-desktop/` vendor patch; use each package's owned
validation instead.

## Documentation Guidelines

Keep `README.md` concise and English-only. Use it as the project entry point and
link to focused documents instead of embedding detailed operation or
implementation material. Put the feature tour and shortcuts in `FEATURES.md`,
deployment and configuration instructions in `docs/DEPLOYMENT.md`, and system
contracts in `docs/ARCHITECTURE.md`. Permanent docs describe current supported
behavior and contracts, not task status, plans, phases, dated verification logs,
or agent transcripts; keep those details in PRs, external artifacts, or Git
history. Add a focused document only when an enduring topic cannot fit an
existing home. Keep one canonical home per topic and link to it. When finishing
work, consolidate or delete stale status documents and repair their links.

## Testing Guidelines

Unit tests live beside their modules as `*.test.ts` and use `bun:test`. Run
`bun run precommit` before committing. For frontend-facing work, also run
`bun run build:web`. Release work must package and inspect every supported
platform archive and checksum. Android releases target `arm64-v8a`; APK
validation must assert that native library and package Vite assets at the
Android asset root before publishing.

Pre-commit preserves Git's staged index for prek, then clears repository-local
Git environment variables before Bun tests so temporary repositories stay isolated.

## Commit & Pull Request Guidelines

For this refactor, gather worker implementation into shprd-refactor while preserving
coordinator fixes, validate and code-review the combined changes, then commit and
push to origin/shprd-refactor. Treat this branch as work in progress until full
parity is verified. Do not create PRs, modify main, rewrite worker history, or
remove worker worktrees as part of this workflow.

Git history uses concise imperative messages, for example `Use built-in CLI
argument parser` or `Add command palette and release 0.0.3`. Keep commits
focused and mention user-visible behavior in the message when relevant. PRs
should include a short summary, verification commands, and screenshots for UI
changes.

## Release & Changelog Notes

Keep `CHANGELOG.md` English-only with short user-facing highlights and important
fixes, normally 3-5 bullets per release. Collapse repetition; omit implementation
names, internal flows, and verification narratives. Preserve migration, security,
breaking-change, data-loss, and platform-compatibility essentials even when more
space is needed. Add entries under `## Unreleased`; retain historical version/date
headings and their order when editing. Leave detailed records in PRs, external
artifacts, or Git history, not the changelog.

Stable releases use separate prepare and publish phases:

1. Run the **Prepare Release** workflow with `X.Y.Z` or
   `patch`/`minor`/`major`. It updates the three `package.json` versions,
   workspace Cargo version, and `herdr-plugin.toml`, finalizes `CHANGELOG.md`
   from `## Unreleased`, and opens a normal release PR.
   Review and merge that PR after its checks pass; the workflow never merges or
   tags on its own.
2. Run the **Publish Release** workflow with the merged `X.Y.Z` version. It
   finds and verifies the matching release commit on `main`, creates the
   annotated `vX.Y.Z` tag there, and starts `.github/workflows/release.yml` on
   that tag.

For local preparation, run `bun run release:check-bump <X.Y.Z | patch | minor | major>`
before `bun run release:prepare <X.Y.Z | patch | minor | major>`. Releases must
advance one SemVer boundary: from `0.8.0`, valid inputs resolve only to `0.8.1`,
`0.9.0`, or `1.0.0`. Use patch by default for compatible work. Minor and major
releases require `--allow-non-patch` after confirming a new backward-compatible
public capability or breaking public contract. Conventional Commit output does
not select a release class. Release preparation updates files without committing,
tagging, or pushing. Submit those changes through a normal PR because direct
pushes to `main` are not allowed.

Public release notes are generated by GitHub from merged pull requests using
`--generate-notes` and `.github/release.yml`; `CHANGELOG.md` remains the concise
in-app history and receives version headings only in release PRs.

# DOX framework

- DOX is highly performant AGENTS.md hierarchy installed here
- Agent must follow DOX instructions across any edits

## Core Contract

- AGENTS.md files are binding work contracts for their subtrees
- Work products, source materials, instructions, records, assets, and durable docs must stay understandable from the nearest applicable AGENTS.md plus every parent AGENTS.md above it

## Read Before Editing

1. Read the root AGENTS.md
2. Identify every file or folder you expect to touch
3. Walk from the repository root to each target path
4. Read every AGENTS.md found along each route
5. If a parent AGENTS.md lists a child AGENTS.md whose scope contains the path, read that child and continue from there
6. Use the nearest AGENTS.md as the local contract and parent docs for repo-wide rules
7. If docs conflict, the closer doc controls local work details, but no child doc may weaken DOX

Do not rely on memory. Re-read the applicable DOX chain in the current session before editing.

## Update After Editing

Every meaningful change requires a DOX pass before the task is done.

Update the closest owning AGENTS.md when a change affects:

- purpose, scope, ownership, or responsibilities
- durable structure, contracts, workflows, or operating rules
- required inputs, outputs, permissions, constraints, side effects, or artifacts
- user preferences about behavior, communication, process, organization, or quality
- AGENTS.md creation, deletion, move, rename, or index contents

Update parent docs when parent-level structure, ownership, workflow, or child index changes. Update child docs when parent changes alter local rules. Remove stale or contradictory text immediately. Small edits that do not change behavior or contracts may leave docs unchanged, but the DOX pass still must happen.

## Hierarchy

- Root AGENTS.md is the DOX rail: project-wide instructions, global preferences, durable workflow rules, and the top-level Child DOX Index
- Child AGENTS.md files own domain-specific instructions and their own Child DOX Index
- Each parent explains what its direct children cover and what stays owned by the parent
- The closer a doc is to the work, the more specific and practical it must be

## Child Doc Shape

- Create a child AGENTS.md when a folder becomes a durable boundary with its own purpose, rules, responsibilities, workflow, materials, or quality standards
- Work Guidance must reflect the current standards of the project or user instructions; if there are no specific standards or instructions yet, leave it empty
- Verification must reflect an existing check; if no verification framework exists yet, leave it empty and update it when one exists

Default section order:
- Purpose
- Ownership
- Local Contracts
- Work Guidance
- Verification
- Child DOX Index

## Style

- Keep docs concise, current, and operational
- Document stable contracts, not diary entries
- Put broad rules in parent docs and concrete details in child docs
- Prefer direct bullets with explicit names
- Do not duplicate rules across many files unless each scope needs a local version
- Delete stale notes instead of explaining history
- Trim obvious statements, repeated rules, misplaced detail, and warnings for risks that no longer exist

## Closeout

1. Re-check changed paths against the DOX chain
2. Update nearest owning docs and any affected parents or children
3. Refresh every affected Child DOX Index
4. Remove stale or contradictory text
5. Run existing verification when relevant
6. Report any docs intentionally left unchanged and why

## User Preferences

When the user requests a durable behavior change, record it here or in the relevant child AGENTS.md

## Child DOX Index

- `.agents/` — tracked agent-skill sources and the root `skills-lock.json` resolution lock. `.agents/AGENTS.md` owns imported-skill preservation and child skill indexes.
- `.config/`, `.pi/`, `.qlty/`, `.omo/` — local tool configuration and agent state. Root owns contracts; do not commit generated state unless explicitly requested.
- `.githooks/` — repository commit and pre-commit hooks. Root owns hook contracts.
- `.github/` — CI, release, and repository automation. Root owns workflow contracts.
- `.hermes/` — tracked external-integration aliases. Root owns the compatibility contract; aliases resolve through `.agents/`.
- `crates/` — experimental Rust host, shell, shared native libraries, and a minimal local Dioxus 0.7.10 patch. Root owns cross-crate contracts; local crate docs add details where present.
- `deploy/` — service definitions and install defaults. Root owns platform parity.
- `docs/` — permanent architecture and operator documentation plus tracked implementation plans. `docs/AGENTS.md` owns documentation boundaries.
- `integrations/` — Pi adapter glue and engine integrations. Root owns extension contracts.
- `scripts/` — build, installer, packaging, and release helpers. Root owns release contracts.
- `server/` — Bun bridge, local service management, and generated public artifacts. Root owns bridge contracts.
- `site/` — static documentation site. Root owns published-site behavior.
- `web/` — React/Vite workspace UI. Root owns browser contracts.
