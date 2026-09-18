import { describe, expect, test } from "bun:test";
import { homedir } from "node:os";
import { join } from "node:path";
import {
  herdrConfigDir,
  isTailnetIPv4,
  nativeSocketPath,
  resolveAuthRequired,
  resolveServerLogLevel,
} from "./server-config";

describe("herdrConfigDir", () => {
  test("uses APPDATA on win32", () => {
    const appData = join("C:", "AppData", "Roaming");
    expect(herdrConfigDir("win32", appData)).toBe(join(appData, "herdr"));
  });

  test("falls back under the home directory on win32 without APPDATA", () => {
    expect(herdrConfigDir("win32", null)).toBe(
      join(homedir(), "AppData", "Roaming", "herdr"),
    );
  });

  test("uses the XDG-style config dir on other platforms", () => {
    expect(herdrConfigDir("darwin")).toBe(join(homedir(), ".config", "herdr"));
    expect(herdrConfigDir("linux")).toBe(join(homedir(), ".config", "herdr"));
  });
});

describe("resolveAuthRequired", () => {
  test("allows ACL-only auth only on Tailnet IPv4", () => {
    expect(isTailnetIPv4("100.85.194.64")).toBe(true);
    expect(isTailnetIPv4("100.128.0.1")).toBe(false);
    expect(resolveAuthRequired("100.85.194.64", true)).toBe(false);
    expect(resolveAuthRequired("100.85.194.64", false)).toBe(true);
    expect(() => resolveAuthRequired("0.0.0.0", true)).toThrow(
      "requires HOST to be a Tailnet IPv4",
    );
  });
});

describe("resolveServerLogLevel", () => {
  test("prefers the CLI value over the environment", () => {
    expect(resolveServerLogLevel("debug", "error")).toBe("debug");
  });

  test("uses the environment and defaults to info", () => {
    expect(resolveServerLogLevel(undefined, "warn")).toBe("warn");
    expect(resolveServerLogLevel(undefined, undefined)).toBe("info");
  });
});

describe("nativeSocketPath", () => {
  test("maps Herdr's Windows socket name onto its named pipe", () => {
    const logical = String.raw`C:\AppData\Roaming\herdr\herdr.sock`;
    const native = String.raw`\\.\pipe\C:\AppData\Roaming\herdr\herdr.sock`;

    expect(nativeSocketPath(logical, "win32")).toBe(native);
    expect(nativeSocketPath(native, "win32")).toBe(native);
    const upperPrefix = String.raw`\\.\PIPE\existing`;
    expect(nativeSocketPath(upperPrefix, "win32")).toBe(upperPrefix);
    expect(nativeSocketPath(logical, "linux")).toBe(logical);
  });
});
