import { expect, test } from "bun:test";
import {
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("pre-commit keeps fixture Git writes outside committing repository", async () => {
  const root = await mkdtemp(join(tmpdir(), "shprd-hook-test-"));
  const source = join(root, "source");
  const repository = join(root, "repository");
  const fixture = join(root, "fixture");
  const bin = join(root, "bin");
  const hooks = join(root, "hooks");
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([name]) => !name.startsWith("GIT_")),
  );
  async function run(cwd: string, ...args: string[]) {
    const child = Bun.spawn(args, { cwd, env, stdout: "pipe", stderr: "pipe" });
    const [code, stdout, stderr] = await Promise.all([
      child.exited,
      new Response(child.stdout).text(),
      new Response(child.stderr).text(),
    ]);
    if (code !== 0) throw new Error(`${args.join(" ")}: ${stderr}${stdout}`);
    return stdout.trim();
  }
  try {
    // Given: separate real repositories and actual hook with bounded test commands.
    for (const directory of [source, fixture, bin, hooks]) {
      await mkdir(directory);
    }
    for (const directory of [source, fixture]) {
      await run(directory, "git", "init", "-q");
      await run(directory, "git", "config", "user.name", "Hook Test");
      await run(
        directory,
        "git",
        "config",
        "user.email",
        "hook@example.invalid",
      );
      await run(directory, "git", "config", "commit.gpgsign", "false");
      await run(
        directory,
        "git",
        "-c",
        "core.hooksPath=",
        "commit",
        "--allow-empty",
        "-qm",
        "base",
      );
    }
    await run(source, "git", "worktree", "add", "-qb", "linked", repository);
    const base = await run(repository, "git", "rev-parse", "HEAD");
    await copyFile(
      new URL("../.githooks/pre-commit", import.meta.url),
      join(hooks, "pre-commit"),
    );
    await chmod(join(hooks, "pre-commit"), 0o755);
    await writeFile(
      join(bin, "mise"),
      `#!/bin/sh
set -eu
case "$3" in
  prek) git diff --cached --name-only | grep -qx parent.txt ;;
  bun)
    git -C "$FIXTURE_REPO" add child.txt
    git -C "$FIXTURE_REPO" -c core.hooksPath= commit -qm fixture
    ;;
  *) exit 2 ;;
esac
`,
      { mode: 0o755 },
    );
    env.PATH = `${bin}:${env.PATH ?? ""}`;
    env.FIXTURE_REPO = fixture;
    await writeFile(join(repository, "parent.txt"), "parent\n");
    await writeFile(join(fixture, "child.txt"), "child\n");
    await run(repository, "git", "add", "parent.txt");

    // When: Git invokes the real hook during an outer commit.
    await run(
      repository,
      "git",
      "-c",
      `core.hooksPath=${hooks}`,
      "commit",
      "-qm",
      "parent",
    );

    // Then: fixture and parent commits stay in their own repositories.
    expect(await run(repository, "git", "rev-parse", "HEAD^")).toBe(base);
    expect(await run(repository, "git", "ls-tree", "--name-only", "HEAD")).toBe(
      "parent.txt",
    );
    expect(await run(fixture, "git", "ls-tree", "--name-only", "HEAD")).toBe(
      "child.txt",
    );
    expect(await run(repository, "git", "status", "--porcelain")).toBe("");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
