# Release and commit policy

## Commits

Commit headers MUST use `<type>[optional scope][!]: <description>`. Allowed types are `build`, `chore`, `ci`, `docs`, `feat`, `fix`, `perf`, `refactor`, `revert`, `style`, and `test`. Scopes, when present, MUST be lowercase kebab-case. Headers MUST be 100 characters or fewer.

Use `feat` for SemVer minor changes and `fix` for SemVer patch changes. Mark breaking changes with `!` or a `BREAKING CHANGE:` footer; these require a SemVer major change. Other types do not trigger a release unless marked breaking.

Prek owns all local Git hooks. Install its `pre-commit`, `commit-msg`, and `pre-push` shims with `mise run install-hooks`. Do not use `qlty githooks install`. CI invokes quality commands directly and does not depend on installed hooks.

## Releases

Repository maintainers are sole version and tag authority. A release tag MUST be an annotated `vMAJOR.MINOR.PATCH` tag created only after `mise run ci` passes. Automation MUST NOT create or push tags. Package versions are independent of Dioxus framework versions: upstream Cargo versions and tags describe compatibility evidence, not this repository's release number. Source SHAs in provenance are authoritative for behavior; neither the generated catalog nor `skills-lock.json` defines a package version.

Generate release notes from a reviewed, explicit Git range with `mise run changelog -- <FROM>..<TO>`. `cliff.toml` requires Conventional Commits, fixed grouping, and oldest-first ordering. Maintainers review generated output before release publication.

Release procedure:

1. Review the complete commit range, breaking changes, provenance, and compatibility claims. Select the package version manually.
2. Run `mise install --locked` and `mise run ci` against the selected clean Dioxus source revision. Inspect the report even when validation succeeds.
3. Generate notes from explicit immutable endpoints; update `CHANGELOG.md` according to its policy. The fixed-history fixture in `mise run validate-tooling` checks repeatable output.
4. Obtain maintainer approval of the final clean package revision, version, notes, and evidence. An authorized maintainer creates the annotated tag and publishes notes separately; this repository has no automatic release/tag job.

`apply` is implemented but always explicit. It never runs from `check` or CI.
GitHub Actions validates every PR commit, uploads inspection evidence, and has only
`contents: read`. Failed quality gates prevent release; artifacts are evidence,
not authorization to mutate source or publish.
