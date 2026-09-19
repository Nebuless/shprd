import { afterEach, describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const temporaryRoots: string[] = [];

function currentReleasePlatform(): string {
  if (process.platform === "darwin" && process.arch === "arm64") {
    return "darwin-arm64";
  }
  if (process.platform === "darwin" && process.arch === "x64") {
    return "darwin-x64";
  }
  if (process.platform === "linux" && process.arch === "arm64") {
    return "linux-arm64";
  }
  if (process.platform === "linux" && process.arch === "x64") {
    return "linux-x64";
  }
  throw new Error(
    `unsupported test platform: ${process.platform}-${process.arch}`,
  );
}

function createInstallerFixture(checksumName?: string) {
  const root = mkdtempSync(join(tmpdir(), "shprd-installer-test-"));
  temporaryRoots.push(root);
  const assets = join(root, "assets");
  const fakeBin = join(root, "bin");
  const installDir = join(root, "install");
  const platform = currentReleasePlatform();
  const packageDir = `shprd-${platform}`;
  const archiveName = `${packageDir}.tar.xz`;
  const packagePath = join(assets, packageDir);
  mkdirSync(packagePath, { recursive: true });
  mkdirSync(fakeBin, { recursive: true });

  const binary = join(packagePath, "shprd");
  writeFileSync(
    binary,
    [
      "#!/bin/sh",
      'if [ "${1:-}" = "--version" ]; then',
      '  echo "shprd 9.8.7"',
      "  exit 0",
      "fi",
      'if [ "${1:-}" = "service" ] && [ "${2:-}" = "install" ]; then',
      '  [ -z "${SERVICE_EXEC_LOG:-}" ] || printf "%s %s\\n" "$0" "$*" >> "$SERVICE_EXEC_LOG"',
      "  exit 0",
      "fi",
      "exit 1",
      "",
    ].join("\n"),
    { mode: 0o755 },
  );
  writeFileSync(join(packagePath, "VERSION"), `shprd 9.8.7 ${platform}\n`);

  const archive = join(assets, archiveName);
  const packaged = Bun.spawnSync(
    ["tar", "-C", assets, "-cJf", archive, packageDir],
    {
      env: { ...process.env, COPYFILE_DISABLE: "1" },
      stderr: "pipe",
    },
  );
  if (packaged.exitCode !== 0) {
    throw new Error(packaged.stderr.toString());
  }
  const digest = createHash("sha256")
    .update(readFileSync(archive))
    .digest("hex");
  writeFileSync(
    `${archive}.sha256`,
    `${digest}  ${checksumName ?? archiveName}\n`,
  );

  const fakeCurl = join(fakeBin, "curl");
  writeFileSync(
    fakeCurl,
    [
      "#!/bin/sh",
      "set -eu",
      'out=""',
      'url=""',
      'while [ "$#" -gt 0 ]; do',
      '  case "$1" in',
      '    -o) out="$2"; shift 2 ;;',
      "    -*) shift ;;",
      '    *) url="$1"; shift ;;',
      "  esac",
      "done",
      '[ -z "${CURL_OUTPUT_LOG:-}" ] || printf "%s\\n" "$out" >> "$CURL_OUTPUT_LOG"',
      'if [ -n "${FAIL_CURL_PREFIX:-}" ]; then',
      '  case "$out" in',
      '    "${FAIL_CURL_PREFIX}"/*) exit 23 ;;',
      "  esac",
      "fi",
      'cp "$FIXTURE_DIR/${url##*/}" "$out"',
      "",
    ].join("\n"),
    { mode: 0o755 },
  );
  chmodSync(fakeCurl, 0o755);

  return { root, assets, fakeBin, installDir };
}

function runInstaller(
  fixture: ReturnType<typeof createInstallerFixture>,
  releaseBaseUrl = "http://127.0.0.1/releases",
  environment: Record<string, string> = {},
) {
  return Bun.spawnSync(["sh", join(import.meta.dir, "install-shprd.sh")], {
    env: {
      ...process.env,
      PATH: `${fixture.fakeBin}:${process.env.PATH ?? ""}`,
      FIXTURE_DIR: fixture.assets,
      SHPRD_RELEASE_BASE_URL: releaseBaseUrl,
      SHPRD_INSTALL_DIR: fixture.installDir,
      ...environment,
    },
    stdout: "pipe",
    stderr: "pipe",
  });
}

afterEach(() => {
  for (const root of temporaryRoots.splice(0)) {
    rmSync(root, { recursive: true, force: true });
  }
});

describe("release installer", () => {
  test("verifies, backs up, and installs the expected platform package", () => {
    const fixture = createInstallerFixture();
    mkdirSync(fixture.installDir, { recursive: true });
    writeFileSync(join(fixture.installDir, "shprd"), "previous binary\n", {
      mode: 0o755,
    });
    const result = runInstaller(fixture);
    expect(result.exitCode).toBe(0);
    expect(result.stderr.toString()).toBe("");

    const installed = Bun.spawnSync(
      [join(fixture.installDir, "shprd"), "--version"],
      { stdout: "pipe" },
    );
    expect(installed.exitCode).toBe(0);
    expect(installed.stdout.toString().trim()).toBe("shprd 9.8.7");
    expect(
      readFileSync(join(fixture.installDir, "shprd.previous"), "utf8"),
    ).toBe("previous binary\n");
  });

  test("starts the persistent installed binary as a user service", () => {
    const fixture = createInstallerFixture();
    const serviceLog = join(fixture.root, "service.log");
    const result = runInstaller(fixture, undefined, {
      SERVICE_EXEC_LOG: serviceLog,
    });

    expect(result.exitCode).toBe(0);
    expect(readFileSync(serviceLog, "utf8")).toBe(
      `${join(fixture.installDir, "shprd")} service install\n`,
    );
  });

  test("rejects filenames supplied by an untrusted checksum file", () => {
    const fixture = createInstallerFixture("../../unrelated-file");
    const result = runInstaller(fixture);
    expect(result.exitCode).not.toBe(0);
    expect(result.stderr.toString()).toContain("invalid package checksum file");
  });

  test("refuses to replace a symlinked install target", () => {
    const fixture = createInstallerFixture();
    mkdirSync(fixture.installDir, { recursive: true });
    const outside = join(fixture.root, "outside-binary");
    writeFileSync(outside, "outside\n", { mode: 0o755 });
    symlinkSync(outside, join(fixture.installDir, "shprd"));

    const result = runInstaller(fixture);
    expect(result.exitCode).not.toBe(0);
    expect(result.stderr.toString()).toContain(
      "install target exists but is not a regular file",
    );
    expect(readFileSync(outside, "utf8")).toBe("outside\n");
  });

  test("uses home staging by default instead of the system temp directory", () => {
    const fixture = createInstallerFixture();
    const systemTemp = join(fixture.root, "system-temp");
    const curlLog = join(fixture.root, "curl.log");

    const result = runInstaller(fixture, undefined, {
      TMPDIR: systemTemp,
      HOME: fixture.root,
      CURL_OUTPUT_LOG: curlLog,
    });

    const homeTemp = join(fixture.root, ".local", "share", "shprd", "tmp");
    expect(result.exitCode).toBe(0);
    expect(result.stderr.toString()).not.toContain(
      "Custom temporary directory unavailable",
    );
    const outputs = readFileSync(curlLog, "utf8");
    expect(outputs).toContain(`${homeTemp}/`);
    expect(outputs).not.toContain(`${systemTemp}/`);
    expect(readdirSync(homeTemp)).toEqual([]);
  });

  test("falls back to home staging when custom staging download fills", () => {
    const fixture = createInstallerFixture();
    const customTemp = join(fixture.root, "custom-temp");
    const curlLog = join(fixture.root, "curl.log");
    mkdirSync(customTemp, { recursive: true });

    const result = runInstaller(fixture, undefined, {
      SHPRD_TEMP_DIR: customTemp,
      HOME: fixture.root,
      FAIL_CURL_PREFIX: customTemp,
      CURL_OUTPUT_LOG: curlLog,
    });

    const homeTemp = join(fixture.root, ".local", "share", "shprd", "tmp");
    expect(result.exitCode).toBe(0);
    const outputs = readFileSync(curlLog, "utf8");
    expect(outputs).toContain(`${customTemp}/`);
    expect(outputs).toContain(`${homeTemp}/`);
    expect(readdirSync(homeTemp)).toEqual([]);
  });

  test("rejects unauthenticated non-loopback release mirrors", () => {
    const fixture = createInstallerFixture();
    const result = runInstaller(fixture, "http://downloads.example.com/shprd");
    expect(result.exitCode).not.toBe(0);
    expect(result.stderr.toString()).toContain(
      "release base URL must use HTTPS unless the mirror is loopback",
    );
  });
});
