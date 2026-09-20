import { describe, expect, test } from "bun:test";
import { isAbsolute, join } from "node:path";

function unzip(...arguments_: readonly string[]): string {
  const result = Bun.spawnSync(["unzip", ...arguments_]);
  if (result.exitCode !== 0) {
    throw new Error(result.stderr.toString().trim());
  }
  return result.stdout.toString();
}

const configuredApk = process.env.SHPRD_ANDROID_APK;
const apk = configuredApk
  ? isAbsolute(configuredApk)
    ? configuredApk
    : join(process.cwd(), configuredApk)
  : "";
const hasApk = configuredApk !== undefined && (await Bun.file(apk).exists());

describe("Android release package", () => {
  test.skipIf(!hasApk)(
    "opens the retained React entry from packaged assets",
    () => {
      // Given: the APK produced by an Android build route.

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
      expect(entries).toContain("lib/arm64-v8a/libmain.so");
      expect(assetPaths.length).toBeGreaterThan(1);
      for (const assetPath of assetPaths) {
        expect(entries).toContain(join(indexEntry, "..", assetPath));
      }
    },
  );
});
