#!/usr/bin/env bun

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  assertPatchReleaseOrOverride,
  classifyVersionBump,
} from "./release-bump";
import { parsePackageVersion, resolveNextVersion } from "./prepare-release";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));

function abort(message: string): never {
  console.error(`check-release-bump: ${message}`);
  process.exit(1);
}

function main(): void {
  const [input, override] = process.argv.slice(2);
  if (!input || input === "--help") {
    console.log(
      "Usage: bun scripts/check-release-bump.ts <X.Y.Z | patch | minor | major> [--allow-non-patch]",
    );
    process.exit(input ? 0 : 1);
  }
  if (override && override !== "--allow-non-patch") {
    abort(`unexpected argument ${override}`);
  }
  if (process.argv.length > 4) abort("unexpected extra arguments");

  const packageJson = readFileSync(join(REPO_ROOT, "package.json"), "utf8");
  const current = parsePackageVersion(packageJson);
  const candidate = resolveNextVersion(current, input);
  const bump = classifyVersionBump(current, candidate);
  assertPatchReleaseOrOverride(
    current,
    candidate,
    override === "--allow-non-patch",
  );
  console.log(`${candidate} is the next ${bump} version after ${current}.`);
}

try {
  main();
} catch (error) {
  abort(error instanceof Error ? error.message : String(error));
}
