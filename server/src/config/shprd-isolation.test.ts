import { afterEach, expect, test } from "bun:test";
import { homedir } from "node:os";
import { join } from "node:path";
import { defaultConnectionProfilesPath } from "../connections/profiles";
import { createAuthHandlers } from "../http/auth";
import { defaultAuthTokenPath } from "./auth-token";
import { guiSettingsPath } from "./gui-settings";

const originalConfigDir = process.env.SHPRD_CONFIG_DIR;
const originalConnectionsPath = process.env.HERDR_GUI_CONNECTIONS_PATH;

afterEach(() => {
  if (originalConfigDir === undefined) delete process.env.SHPRD_CONFIG_DIR;
  else process.env.SHPRD_CONFIG_DIR = originalConfigDir;
  if (originalConnectionsPath === undefined)
    delete process.env.HERDR_GUI_CONNECTIONS_PATH;
  else process.env.HERDR_GUI_CONNECTIONS_PATH = originalConnectionsPath;
});

test("SHPRD config directory isolates durable files without changing legacy defaults", () => {
  // Given separate legacy and branded installations.
  delete process.env.SHPRD_CONFIG_DIR;
  delete process.env.HERDR_GUI_CONNECTIONS_PATH;
  const legacyDir = join(homedir(), ".config", "herdr-gui");
  expect(guiSettingsPath()).toBe(join(legacyDir, "settings.json"));
  expect(defaultConnectionProfilesPath()).toBe(
    join(legacyDir, "connections.json"),
  );
  expect(defaultAuthTokenPath("/home/test", "linux")).toBe(
    join("/home/test", ".config", "herdr-gui", "auth-token"),
  );
  const shprdDir = join(homedir(), ".config", "shprd", "studio");

  // When only the branded config directory is selected.
  process.env.SHPRD_CONFIG_DIR = shprdDir;

  // Then every app-owned config path is isolated; explicit registry paths still win.
  expect(defaultAuthTokenPath()).toBe(join(shprdDir, "auth-token"));
  expect(guiSettingsPath()).toBe(join(shprdDir, "settings.json"));
  expect(defaultConnectionProfilesPath()).toBe(
    join(shprdDir, "connections.json"),
  );
  process.env.HERDR_GUI_CONNECTIONS_PATH = join(shprdDir, "custom.json");
  expect(defaultConnectionProfilesPath()).toBe(join(shprdDir, "custom.json"));
});

test("SHPRD login cookie coexists with legacy login on another port", () => {
  // Given both installations on one hostname and different ports.
  delete process.env.SHPRD_CONFIG_DIR;
  const legacy = createAuthHandlers({
    authRequired: true,
    password: "legacy-secret",
    urlLoginToken: "legacy-secret",
  });
  process.env.SHPRD_CONFIG_DIR = join(homedir(), ".config", "shprd", "studio");
  const shprd = createAuthHandlers({
    authRequired: true,
    password: "shprd-secret",
    urlLoginToken: "shprd-secret",
  });

  // When each login issues its cookie.
  const legacyLogin = legacy.handleTokenLogin(
    new Request("http://example.test:18777/?token=legacy-secret"),
  );
  const shprdLogin = shprd.handleTokenLogin(
    new Request("http://example.test:18778/?token=shprd-secret"),
  );
  const legacyCookie = legacyLogin?.headers.get("set-cookie")?.split(";")[0];
  const shprdCookie = shprdLogin?.headers.get("set-cookie")?.split(";")[0];

  // Then browsers retain both sessions, with no cross-authorization.
  expect(legacyCookie?.startsWith("herdr_auth=")).toBe(true);
  expect(shprdCookie?.startsWith("shprd_auth=")).toBe(true);
  const both = new Request("http://example.test/", {
    headers: { cookie: `${legacyCookie}; ${shprdCookie}` },
  });
  expect(legacy.isAuthed(both)).toBe(true);
  expect(shprd.isAuthed(both)).toBe(true);
  expect(
    shprd.isAuthed(
      new Request("http://example.test/", {
        headers: { cookie: `${legacyCookie}` },
      }),
    ),
  ).toBe(false);
  expect(
    legacy.isAuthed(
      new Request("http://example.test/", {
        headers: { cookie: `${shprdCookie}` },
      }),
    ),
  ).toBe(false);
});
