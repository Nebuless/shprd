# Contributing

## Development Setup

Install [mise](https://mise.jdx.dev/) and provision pinned local tools and dependencies:

```bash
mise install
mise run install
mise run hooks:install
```

Mise supplies Bun, Worktrunk (`wt`), Prek, Qlty, ShellCheck, formatting and
workflow tools, plus language servers for TypeScript, JSON/CSS/HTML/ESLint,
YAML, TOML, Markdown, and shell scripts. Editors resolve these from the project
mise environment.

Run the bridge and frontend in separate terminals:

```bash
bun run dev:server
bun run dev:web
```

## Validation

Before submitting a change, run local CI:

```bash
mise run ci
```

Use individual `mise run format`, `lint`, `typecheck`, `test`, `site`,
`check:languages`, `hooks`, or `quality` tasks while iterating. Qlty reports
maintainability smells in `.qlty/qlty.toml`; Biome and ESLint remain format and
lint authority.

Use `bun run build` for changes that affect production assets or server
bundling. Release changes should also validate the relevant
`package:<platform>` command.

## Pages Website and Tutorial

The landing page lives in `site/`. The tutorial has one canonical
source, `docs/TUTORIAL.md`; `scripts/build-pages.ts` renders it into the
`site/tutorial/index.html` template, rewrites shared screenshots and reference
links, and validates local links and fragments throughout the built site.
Do not duplicate the tutorial body in the HTML template.

```bash
bun test scripts/pages-content.test.ts
bun run build:site
```

Serve `.pages-dist/` with a local static HTTP server and open `/tutorial/`.
Also check deployment beneath the `/shprd/` Pages subpath, narrow-screen
layouts, keyboard navigation, and reading with JavaScript disabled. Generated
`.pages-dist/` files must not be committed. The Pages workflow rebuilds when
the tutorial source, renderer, template, or shared website assets change.

## Pull Requests

Keep commits focused and use [Conventional Commits](https://www.conventionalcommits.org/)
messages such as `feat: add session import` or `fix(server): close stale socket`.
The `commit-msg` hook checks each message, and CI checks every PR commit range.
Describe user-visible behavior, verification performed, and compatibility impact.
Include screenshots for interface changes. Avoid committing generated
artifacts from `dist/`, `server/public/`, or compiled binaries.

Pull requests without a release-note category label are labeled automatically:
documentation-only changes become `documentation`, dependency updates become
`dependencies`, fix-oriented titles become `bug`, and other code changes become
`enhancement`. Release preparation PRs receive `skip-changelog`. Add one of the
categories from `.github/release.yml` before merging to override the automatic
choice.

Use `bun run release:check-bump <X.Y.Z|patch|minor|major>` before preparing a
release. Releases advance one SemVer boundary at a time: from `0.8.0`, only
`0.8.1`, `0.9.0`, or `1.0.0` are valid. Use `patch` for compatible fixes,
polish, documentation, and internal work. Minor and major releases require
`--allow-non-patch` after confirming a new backward-compatible public capability
or breaking public contract. Conventional Commit output does not choose the
release class. `bun run changelog:preview` previews generated notes only. Keep
`CHANGELOG.md` concise and maintained by release preparation.

Worktrunk runs `mise run install` for each new worktree through `.config/wt.toml`.

By contributing, you agree that your contribution is licensed under the MIT
License.
