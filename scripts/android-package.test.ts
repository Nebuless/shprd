import { describe, expect, test } from "bun:test";
import { isAbsolute, join } from "node:path";

const defaultApk = join(
  import.meta.dir,
  "../target/dx/shprd-shell/release/android/app/app/build/outputs/apk/release/app-release-unsigned.apk",
);

function unzip(...arguments_: readonly string[]): string {
  const result = Bun.spawnSync(["unzip", ...arguments_]);
  if (result.exitCode !== 0) {
    throw new Error(result.stderr.toString().trim());
  }
  return result.stdout.toString();
}

describe("Android release package", () => {
  test("opens the retained React entry from packaged assets", () => {
    // Given: the APK produced by the release build route.
    const configuredApk = process.env.SHPRD_ANDROID_APK;
    const apk = configuredApk
      ? isAbsolute(configuredApk)
        ? configuredApk
        : join(process.cwd(), configuredApk)
      : defaultApk;

    // When: a consumer opens the packaged Vite entry.
    const entries = unzip("-Z1", apk).trim().split("\n");
    const indexEntry = entries.find((entry) => entry.endsWith("/index.html"));
    expect(indexEntry).toBeDefined();
    if (!indexEntry) return;
    const index = unzip("-p", apk, indexEntry);
    const assetPaths = [...index.matchAll(/(?:src|href)="\.\/([^"]+)"/g)].map(
      (match) => match[1],
    );

    // Then: Vite's root and every directly referenced static asset exist in the APK.
    expect(index).toContain('<div id="root"></div>');
    expect(assetPaths.length).toBeGreaterThan(1);
    for (const assetPath of assetPaths) {
      expect(entries).toContain(join(indexEntry, "..", assetPath));
    }
  });
});
