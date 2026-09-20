# Read-only provenance reports

Run `mise run report -- --source /root/repo/dioxus --json-out reports/report.json`.
Run `mise run validate -- --report reports/report.json` for offline structural and integrity validation.
Add `--source /root/repo/dioxus --target "$PWD"` to validation to reject live source or target drift.

Report accepts `--target DIR` and `--provenance FILE`. Default mapping is
`TARGET/generated/provenance.json`, when present. Each ledger artifact produces one
operation. Unmapped files under `skills/`, `references/`, `assets/`, and `templates/`
produce blocked operations rather than invented source pins. Root `SKILL.md` is
package routing metadata, outside version-one Dioxus artifact roots.

Current revisions supply expected artifact bytes' hashes. Existing identical files
are `noop`; files matching the retained direct supersession predecessor are
approved `update` when source checks pass; other differing files are blocked
`update`; absent files are `add`. These are
inspection proposals, not generated guidance or authorization to apply. Source
hashes cover whole file bytes; locators additionally identify relevant symbols or
line ranges. Source changes require human-authored guidance review. Upstream
differences block operations while local source retains authority.

Default persistent comparison is `~/.cache/dioxus-skills/upstream.git`, a dedicated
bare clone, outside source and target roots. A sibling advisory lock remains held
through fetch and inspection. Existing comparison origins must match. Fetch uses
origin HEAD and reads immutable Git blobs without checkout. Existing worktree
clones are rejected; choose a new bare-clone path rather than converting them.
Use `--comparison PATH` to select another cache, `--no-upstream` to disable network.
Unavailable network or busy lock fails; no stale-cache fallback is implied.

Both output forms contain complete, identical operations and evidence. The compact
JSON uses sorted keys. `createdAt` defaults to local HEAD commit time, not wall
clock; `--created-at YYYY-MM-DDTHH:MM:SSZ` supplies an explicit timestamp. Report ID
hashes report content excluding its own ID. Inspection evidence occupies the
version-one validation check named `inspection-v1`; its detail is canonical JSON.
This preserves the existing report schema. Integrity hash detects accidental
tampering, not malicious resigning; it is not a signature.

Source snapshots include status, file bytes, remotes, local configuration hash,
submodule state and recursively initialized submodule contents. Snapshots are
checked before and after inspection; source Git commands disable optional locks
and filesystem monitors. Dirty source requires `--allow-dirty`. Reads reject
symlink components and non-regular files. Descriptor-relative reads and report
replacement resist parent symlink substitution. Comparison roots, lock paths,
output paths and ledger paths cannot overlap protected roots. Output inside target
is restricted to `reports/`. Existing output survives failed inspection.

Exit codes: report `0` means report produced, including blocked findings; `2`
means inspection/input/network failure. Validator `0` means report is internally
valid, not apply-approved; `1` means invalid or stale; argparse misuse returns `2`.
Normal validation requires one passing `inspection-v1` and a `sha256:` content ID.
Inspection records the full discovered target manifest, including mapped missing
files. Live validation re-discovers artifact roots and rejects additions, removals,
hash changes, and symlinks, including newly introduced paths.
Source identity conflicts produce ambiguous structured candidates; upstream byte
conflicts record local-source precedence while operations remain blocked.
Legacy schema-only reports require explicit `--legacy-only`; this mode forbids
live source/target checks and never satisfies inspection or apply preconditions.
Run `mise run test -- report` for inspection regression and rejection matrix.

## Explicit Apply

Run `mise run apply -- --report /absolute/path/report.json --confirm` from the
target repository root. These are the only apply options. Apply never commits,
pushes, stages the Git index, deletes existing artifacts, or renames artifacts.
Source uses `DIOXUS_SOURCE` when set, otherwise `/root/repo/dioxus`; reports whose repository identity is an absolute
local path use that read-only checkout instead (including isolated fixtures).
Source and target must be disjoint; the protected Dioxus checkout cannot be a target.

Prepare reviewed replacement bytes at
`generated/apply-blobs/<afterSha256>` and the ledger at
`generated/provenance.json` before capturing the report. Payload paths are derived
from validated hashes, never supplied as executable instructions or report paths.
These files must belong to the clean target baseline. Reports contain no payloads;
missing or mismatched blobs fail closed. No source-to-guidance generator is implied.

Only approved `dioxus-authored` operations under `skills/`, `references/`,
`templates/`, and `assets/` may be applied. Live source snapshots, full discovered
target manifests, provenance, report integrity, and passing validation checks are
required. Symlinks and hardlinked write destinations are rejected. A repository
directory advisory lock excludes concurrent cooperating apply processes.

Apply opens allowlisted destinations without following symlinks, retains original
bytes, checks drift again, then writes and validates output hashes. Packages with a
root `SKILL.md` also run the existing normative/style/link validator. Failure restores
all touched originals and removes only files/directories created by that failed
transaction. This is exception rollback, not crash/power-loss recovery; unrelated
concurrent writers must not operate during apply. No backup or receipt files remain.

The initial target must be clean. An exact completed rerun is read-only and accepted
only when the entire authored manifest matches, all dirty paths are precisely the
report's intended edits, HEAD still contains the recorded predecessor bytes, the
index is untouched, and file modes are unchanged. Other dirty states fail closed.
Exit `0` means applied and validated, or already applied; `2` means rejected or failed.
Run `mise run test` for fixture safety coverage.

## Review, Apply, and Recovery Runbook

1. Select the clean authoritative source checkout. Do not fetch, reset, switch branches, or edit `/root/repo/dioxus`. Author proposed guidance and provenance in this repository only; retain predecessor revisions and prepare content-addressed blobs.
2. Run `mise run report -- --json-out reports/report.json` for upstream comparison, or add `--no-upstream` for explicitly offline inspection. Inspect every operation, conflict, source pin, before/after hash, and validation finding. Offline inspection does not claim upstream freshness.
3. Run `mise run validate -- --report reports/report.json --source /root/repo/dioxus --target "$PWD"`. Validation success proves integrity and freshness, not human approval. Resolve blocked/ambiguous findings and regenerate; never edit report verdicts to bypass review.
4. Obtain explicit approval for the exact report ID and intended authored paths. Prepare a clean target baseline containing reviewed provenance and blobs. Keep the report outside the target repository for apply, since an untracked report makes the target dirty. No unrelated writers may run during apply.
5. Only after approval, run `mise run apply -- --report /absolute/path/report.json --confirm`. Review the resulting diff and run `mise run ci`. Apply does not stage, commit, push, tag, or update upstream. CI never invokes this command.
6. On rejection, preserve the error and report; resolve the cause and generate a fresh report. On a caught transactional failure, original bytes are restored automatically. For interruption or power loss, stop concurrent writers and compare each intended path against the saved clean baseline and report hashes. A maintainer restores only those paths from the baseline, removes only additions proven to belong to the interrupted transaction, and reruns validation. Do not reset the whole repository or touch the source checkout. There is no rollback CLI, persistent backup, or crash-recovery guarantee.

## Comparison Clone Lifecycle

The default comparison cache is a dedicated bare clone, not the authoritative
checkout. Reports create it on first use, verify its origin, and fetch under an
advisory lock on each online run. Never reuse a developer worktree as this cache.
Do not remove a lock while another report is active. To retire a cache, stop its
users, retain any needed evidence, and remove only the explicitly identified
comparison directory and sibling lock. A later report recreates the clone.
Use `--comparison /absolute/path/to/upstream.git` to isolate parallel environments.
Network failure or lock contention fails closed; choose `--no-upstream` explicitly
when offline instead of treating stale cached bytes as fresh evidence.
