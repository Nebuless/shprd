# Purpose

Track agent skill sources approved for this repository.

# Ownership

`.agents/skills/` contains imported skill packages. Root `AGENTS.md` owns project-wide agent policy.

# Local Contracts

- Preserve imported skill content, metadata, and licenses. Repository hooks may normalize trailing whitespace and final newlines.
- Do not commit generated outputs, credentials, or local agent state here.
- `.hermes/skills/` contains tracked aliases to approved packages here; other `.hermes/` integration material remains untracked.
- Root formatter, linter, and hooks exclude imported skill snapshots; run their package-supplied validation instead.

# Work Guidance

Treat imported skills as vendor content. Add or update a package only as a complete, attributable source snapshot.

# Verification

Run the repository validation required by root `AGENTS.md` after changing tracked skill packages.

# Child DOX Index

- `skills/dioxus-skills/` — imported Dioxus framework guidance router and focused leaves.
- `skills/rust-skills/` — imported Rust guidance corpus. Its own `AGENTS.md` defines package-local rules.
