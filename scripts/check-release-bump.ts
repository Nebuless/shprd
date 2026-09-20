#!/usr/bin/env bun

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { classifyVersionBump, recommendedReleaseBump } from "./release-bump";
import { parsePackageVersion, resolveNextVersion } from "./prepare-release";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));

function abort(message: string): never {
  console.error(`check-release-bump: ${message}`);
  process.exit(1);
}

function main(): void {
  const input = process.argv[2];
  if (!input || input === "--help") {
    console.log(
      "Usage: bun scripts/check-release-bump.ts <X.Y.Z | patch | minor | major>",
    );
    process.exit(input ? 0 : 1);
  }
  if (process.argv.length > 3) abort("unexpected extra arguments");

  const packageJson = readFileSync(join(REPO_ROOT, "package.json"), "utf8");
  const current = parsePackageVersion(packageJson);
  const candidate = resolveNextVersion(current, input);
  const bump = classifyVersionBump(current, candidate);
  const recommendation = recommendedReleaseBump(REPO_ROOT);
  const recommendationText = recommendation
    ? ` Conventional Commits recommends ${recommendation}.`
    : "";
  console.log(
    `${candidate} is the next ${bump} version after ${current}.${recommendationText}`,
  );
}

try {
  main();
} catch (error) {
  abort(error instanceof Error ? error.message : String(error));
}
