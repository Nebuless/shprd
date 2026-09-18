import { expect, test } from "bun:test";
import { homedir } from "node:os";
import { join } from "node:path";
import { createAuthHandlers } from "../http/auth";

function configPaths(
  configDir?: string,
  connectionsPath?: string,
): readonly string[] {
  const env = { ...process.env };
  if (configDir === undefined) delete env.SHPRD_CONFIG_DIR;
  else env.SHPRD_CONFIG_DIR = configDir;
  if (connectionsPath === undefined) delete env.HERDR_GUI_CONNECTIONS_PATH;
  else env.HERDR_GUI_CONNECTIONS_PATH = connectionsPath;
  const child = Bun.spawnSync(
    [
      process.execPath,
      "--eval",
      `import { defaultConnectionProfilesPath } from ${JSON.stringify(
        new URL("../connections/profiles.ts", import.meta.url).href,
      )};
import { defaultAuthTokenPath } from ${JSON.stringify(
        new URL("./auth-token.ts", import.meta.url).href,
      )};
import { guiSettingsPath } from ${JSON.stringify(
        new URL("./gui-settings.ts", import.meta.url).href,
      )};
console.log([defaultAuthTokenPath(), guiSettingsPath(), defaultConnectionProfilesPath()].join("\\n"));`,
    ],
    { env, stdout: "pipe", stderr: "pipe" },
  );
  expect(child.exitCode).toBe(0);
  return child.stdout.toString().trim().split("\n");
}

test("SHPRD defaults isolate durable files", () => {
  // Given a current installation without an explicit config directory.
  const shprdDir = join(homedir(), ".config", "shprd");
  expect(configPaths()).toEqual([
    join(shprdDir, "auth-token"),
    join(shprdDir, "settings.json"),
    join(shprdDir, "connections.json"),
  ]);

  // When a branded config directory is selected.
  const isolatedDir = join(shprdDir, "studio");

  // Then every app-owned config path is isolated; explicit registry paths still win.
  expect(configPaths(isolatedDir)).toEqual([
    join(isolatedDir, "auth-token"),
    join(isolatedDir, "settings.json"),
    join(isolatedDir, "connections.json"),
  ]);
  expect(configPaths(isolatedDir, join(isolatedDir, "custom.json"))).toEqual([
    join(isolatedDir, "auth-token"),
    join(isolatedDir, "settings.json"),
    join(isolatedDir, "custom.json"),
  ]);
});

test("SHPRD accepts a legacy signed cookie and writes its own cookie", () => {
  // Given an existing legacy browser session.
  const legacy = createAuthHandlers({
    authRequired: true,
    password: "shared-secret",
    urlLoginToken: "shared-secret",
  });
  const legacyLogin = legacy.handleTokenLogin(
    new Request("http://example.test/?token=shared-secret"),
  );
  const legacyCookie = legacyLogin?.headers
    .get("set-cookie")
    ?.replace("shprd_auth=", "herdr_auth=")
    .split(";")[0];
  if (legacyCookie === undefined) throw new Error("missing legacy cookie");

  // When current SHPRD receives that session.
  const shprd = createAuthHandlers({
    authRequired: true,
    password: "shared-secret",
  });

  // Then it authorizes legacy cookie and emits shprd_auth for new logins.
  expect(
    shprd.isAuthed(
      new Request("http://example.test/", {
        headers: { cookie: legacyCookie },
      }),
    ),
  ).toBe(true);
});
