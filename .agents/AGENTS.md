# Purpose

Track agent skill sources approved for this repository.

# Ownership

`.agents/skills/` contains imported skill packages. Root `AGENTS.md` owns project-wide agent policy.

# Local Contracts

- Preserve imported skill content, metadata, and licenses. Repository hooks may normalize trailing whitespace and final newlines.
- Do not commit generated outputs, credentials, or local agent state here.
- `.hermes/` is separate local integration material and remains untracked.

# Work Guidance

Treat imported skills as vendor content. Add or update a package only as a complete, attributable source snapshot.

# Verification

Run the repository validation required by root `AGENTS.md` after changing tracked skill packages.

# Child DOX Index

- `skills/rust-skills/` — imported Rust guidance corpus. Its own `AGENTS.md` defines package-local rules.
