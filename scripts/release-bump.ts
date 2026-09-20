import { spawnSync } from "node:child_process";
import { join } from "node:path";

const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)$/;

export type ReleaseBump = "patch" | "minor" | "major";

const RELEASE_BUMPS = new Set<ReleaseBump>(["patch", "minor", "major"]);

function parseVersion(version: string): readonly [number, number, number] {
  const match = SEMVER_RE.exec(version);
  if (!match) {
    throw new Error(`Version ${version} must be in X.Y.Z form`);
  }
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

export function classifyVersionBump(
  current: string,
  candidate: string,
): ReleaseBump {
  const currentVersion = parseVersion(current);
  const candidateVersion = parseVersion(candidate);

  if (candidateVersion[0] > currentVersion[0]) return "major";
  if (candidateVersion[0] < currentVersion[0]) {
    throw new Error(`Version ${candidate} must be greater than ${current}`);
  }
  if (candidateVersion[1] > currentVersion[1]) return "minor";
  if (candidateVersion[1] < currentVersion[1]) {
    throw new Error(`Version ${candidate} must be greater than ${current}`);
  }
  if (candidateVersion[2] > currentVersion[2]) return "patch";
  throw new Error(`Version ${candidate} must be greater than ${current}`);
}

export function assertBumpIsIncremental(
  current: string,
  candidate: string,
): void {
  const currentVersion = parseVersion(current);
  const bump = classifyVersionBump(current, candidate);
  const expected =
    bump === "patch"
      ? `${currentVersion[0]}.${currentVersion[1]}.${currentVersion[2] + 1}`
      : bump === "minor"
        ? `${currentVersion[0]}.${currentVersion[1] + 1}.0`
        : `${currentVersion[0] + 1}.0.0`;

  if (candidate !== expected) {
    throw new Error(
      `Version ${candidate} must be the next ${bump} version ${expected} from ${current}`,
    );
  }
}

export function assertPatchReleaseOrOverride(
  current: string,
  candidate: string,
  allowNonPatch: boolean,
): void {
  const bump = classifyVersionBump(current, candidate);
  if (bump !== "patch" && !allowNonPatch) {
    throw new Error(
      `Version ${candidate} is ${bump}; use patch or pass --allow-non-patch after confirming a ${bump} release is required`,
    );
  }
}

export function parseRecommendedReleaseBump(
  output: string,
): ReleaseBump | null {
  const recommendation = output.trim();
  if (!recommendation) return null;
  if (!RELEASE_BUMPS.has(recommendation as ReleaseBump)) {
    throw new Error(
      `conventional-recommended-bump returned invalid recommendation: ${recommendation}`,
    );
  }
  return recommendation as ReleaseBump;
}

export function recommendedReleaseBump(repoRoot: string): ReleaseBump | null {
  const executable = join(
    repoRoot,
    "node_modules",
    ".bin",
    "conventional-recommended-bump",
  );
  const result = spawnSync(executable, ["-p", "conventionalcommits"], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (result.error) {
    throw new Error(
      `could not run conventional-recommended-bump: ${result.error.message}`,
    );
  }
  if (result.status !== 0) {
    throw new Error(
      result.stderr.trim() || "conventional-recommended-bump failed",
    );
  }
  return parseRecommendedReleaseBump(result.stdout);
}
