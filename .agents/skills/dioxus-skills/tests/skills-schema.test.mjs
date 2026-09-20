import assert from "node:assert/strict";
import { cp, mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const validator = path.join(repositoryRoot, "scripts", "validate-skills.mjs");
const fixtures = path.join(repositoryRoot, "tests", "fixtures", "skills");

function validate(target) {
  return spawnSync(process.execPath, [validator, "--root", target], {
    cwd: repositoryRoot,
    encoding: "utf8",
  });
}

for (const [name, rootName, diagnostics] of [
  ["uppercase-name", "Uppercase", ["[normative] .: Skill name 'Uppercase' must be lowercase"]],
  ["name-mismatch", "actual-name", ["[normative] .: Directory name 'actual-name' must match skill name 'different-name'"]],
  ["empty-description", "empty-description", ["[normative] .: Field 'description' must be a non-empty string"]],
  ["oversize-description", "oversize-description", ["[normative] .: Description exceeds 1024 character limit (1026 chars)"]],
  ["missing-skill", "missing-skill", ["[normative] skills/absent: Missing required file: SKILL.md"]],
  ["duplicate-name", "duplicate-name", ["[local] duplicate skill name duplicate-name: ., skills/duplicate-name"]],
  ["broken-reference", "broken-reference", ["[local] .: broken relative reference: references/missing.md"]],
]) {
  test(`rejects ${name} fixture`, () => {
    // Given a package containing one named schema defect
    const target = path.join(fixtures, name, rootName);

    // When validation runs
    const result = validate(target);

    // Then it fails with every expected diagnostic and no unrelated diagnostic
    assert.equal(result.status, 1, result.stderr);
    assert.deepEqual(result.stderr.trim().split("\n"), diagnostics);
  });
}

test("accepts valid package fixture", () => {
  // Given a valid root and leaf package
  const target = path.join(fixtures, "valid-package", "valid-package");

  // When validation runs
  const result = validate(target);

  // Then schema and style checks both pass
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /normative ok: 2 skills/);
  assert.match(result.stdout, /style ok: 2 skills/);
});

test("reports 500-line policy separately from schema", async () => {
  // Given a schema-valid skill with 501 body lines
  const temporaryRoot = await mkdtemp(path.join(tmpdir(), "skills-lines-"));
  const skillRoot = path.join(temporaryRoot, "long-skill");
  await mkdir(skillRoot);
  await writeFile(
    path.join(skillRoot, "SKILL.md"),
    `---\nname: long-skill\ndescription: Checks long skill policy. Use when testing style limits.\nmetadata:\n  invocation: model\n---\n${"line\n".repeat(501)}`,
  );

  // When validation runs
  const result = validate(skillRoot);
  await rm(temporaryRoot, { recursive: true, force: true });

  // Then only local writing style fails
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /\[style\] .*SKILL.md exceeds 500 lines/);
  assert.doesNotMatch(result.stderr, /\[normative\]|\[local\]/);
});

test("reports configured token policy separately from schema", async () => {
  // Given a schema-valid skill exceeding a configured token ceiling
  const temporaryRoot = await mkdtemp(path.join(tmpdir(), "skills-tokens-"));
  const skillRoot = path.join(temporaryRoot, "token-skill");
  await mkdir(skillRoot);
  await writeFile(
    path.join(skillRoot, "SKILL.md"),
    "---\nname: token-skill\ndescription: Checks token policy. Use when testing style limits.\nmetadata:\n  invocation: model\n---\none two three\n",
  );

  // When validation runs with a two-token ceiling
  const result = spawnSync(process.execPath, [validator, "--root", skillRoot, "--max-tokens", "2"], {
    cwd: repositoryRoot,
    encoding: "utf8",
  });
  await rm(temporaryRoot, { recursive: true, force: true });

  // Then only local writing style fails with configured limit
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /\[style\] .*SKILL.md exceeds 2 estimated tokens/);
  assert.doesNotMatch(result.stderr, /\[normative\]|\[local\]/);
});

for (const [scalar, marker] of [[">-", "Folded"], ["|-", "Literal"]]) {
test(`accepts ${marker.toLowerCase()} YAML description`, async () => {
  // Given standards-valid block scalar descriptions
  const temporaryRoot = await mkdtemp(path.join(tmpdir(), "skills-yaml-"));
  const skillRoot = path.join(temporaryRoot, "yaml-description");
  await mkdir(skillRoot);
  await writeFile(
    path.join(skillRoot, "SKILL.md"),
    `---\nname: yaml-description\ndescription: ${scalar}\n  Handles ${marker.toLowerCase()} YAML descriptions.\n  Use when validating standards-compatible frontmatter.\nmetadata:\n  invocation: model\n---\n# ${marker}\n`,
  );

  // When local validation delegates YAML parsing to skills-ref
  const result = validate(skillRoot);
  await rm(temporaryRoot, { recursive: true, force: true });

  // Then normative and style validation pass
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /normative ok: 1 skills/);
  assert.match(result.stdout, /style ok: 1 skills/);
});
}

test("malformed frontmatter fails closed", async () => {
  // Given malformed frontmatter instead of parsed metadata
  const temporaryRoot = await mkdtemp(path.join(tmpdir(), "skills-malformed-"));
  const skillRoot = path.join(temporaryRoot, "malformed");
  await mkdir(skillRoot);
  await writeFile(path.join(skillRoot, "SKILL.md"), "---\nname [broken\n---\n");

  // When validation runs
  const result = validate(skillRoot);
  await rm(temporaryRoot, { recursive: true, force: true });

  // Then parsed-metadata failure is explicit and success output absent
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /\[normative\] .*SKILL.md frontmatter must be a YAML mapping/);
  assert.equal(result.stdout, "");
});

test("fails closed on stale generated index", async () => {
  // Given a valid package plus an unrecognized generated catalog
  const temporaryRoot = await mkdtemp(path.join(tmpdir(), "skills-index-"));
  const source = path.join(fixtures, "valid-package", "valid-package");
  const target = path.join(temporaryRoot, "valid-package");
  await cp(source, target, { recursive: true });
  await mkdir(path.join(target, "generated"));
  await writeFile(path.join(target, "generated", "skills.json"), "{}\n");

  // When validation runs
  const result = validate(target);
  await rm(temporaryRoot, { recursive: true, force: true });

  // Then success output is withheld and stale generated state fails
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /\[local\] generated index is stale or malformed/);
  assert.equal(result.stdout, "");
});

test("real package validates from dirty worktree", async () => {
  // Given repository package files independent of Git cleanliness
  const rootSkill = await readFile(path.join(repositoryRoot, "SKILL.md"), "utf8");

  // When validation runs against filesystem content
  const result = validate(repositoryRoot);

  // Then current package succeeds without consulting Git state
  assert.ok(rootSkill);
  assert.equal(result.status, 0, result.stderr);
});
