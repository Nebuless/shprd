# Dioxus Skills

Source-grounded, model-invoked Agent Skills for Dioxus. Start at [SKILL.md](SKILL.md);
the generated catalog is [generated/skills.json](generated/skills.json).
This repository contains guidance, not a Dioxus release or a source mirror.

## Setup and checks

Install Mise using its official platform instructions, then run from this checkout:

```sh
mise trust
mise install --locked
mise run install-hooks
mise run ci
```

Mise owns tool versions; `mise.lock` pins resolved installations. Prek alone owns
local hooks. Hook installation is optional for running CI-equivalent checks.
Do not install QLTY Git hooks or provision tools with system package managers,
standalone installers, or ad-hoc npm/pip commands.
If unrelated global Mise tools prevent a locked install, use
`MISE_CONFIG_DIR=/absolute/path/to/empty-config mise install --locked` to isolate
global configuration; do not regenerate this repository's lockfile for global tools.

`mise run ci` invokes tests, tooling contracts, non-fixing QLTY checks, skill
validation, live provenance validation, and an offline JSON inspection report.
It never applies changes. Tests exercise mutation safety only in disposable fixtures.
For local development, the read-only source defaults to `/root/repo/dioxus`; override with
`DIOXUS_SOURCE=/absolute/path/to/clean/dioxus mise run ci`.
Use the SHA in `generated/provenance.json`, not an arbitrary latest checkout.
CI checks out that source revision separately and uploads `reports/ci-report.json`.
Source-dependent tests require `DIOXUS_SOURCE` in CI; GitHub Actions supplies its
separate checkout path. Equivalent HTTPS repository URLs with a `.git` suffix or
trailing slash identify the same source repository.
Report generation alone does not mean operations are approved.

## Maintenance

```sh
mise run test
mise run validate-skills
mise run validate -- --all
mise run report -- --no-upstream --json-out reports/report.json
mise run validate -- --report reports/report.json
```

Read [CONTRIBUTING.md](CONTRIBUTING.md) for authority and contribution rules,
[docs/REPORTING.md](docs/REPORTING.md) for inspection and guarded apply,
and [docs/RELEASING.md](docs/RELEASING.md) for human-controlled releases.
Commands are defined in [mise.toml](mise.toml); list them with `mise tasks`.
