# Changelog

## Unreleased

No published package release is asserted by this file.

## Policy

Maintainers generate each release section with
`mise run changelog -- FROM..TO`, using reviewed immutable endpoints. Review output
before inserting it above older releases. Keep existing published sections intact;
document corrections explicitly. Do not generate notes from an implicit moving HEAD
or use current wall-clock time as release evidence.

Conventional Commit categories and ordering come from `cliff.toml`. Breaking changes
require a major version, features a minor version, fixes a patch version. See
[release authority](docs/RELEASING.md). A changelog heading is not release authority;
only a maintainer-approved annotated package tag establishes a released version.
