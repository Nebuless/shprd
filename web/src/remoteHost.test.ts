import { afterEach, describe, expect, test } from "bun:test";
import { apiUrl, bridgeWebSocketUrl } from "./remoteHost";

const originalWindow = globalThis.window;

function installHost(host?: string) {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { __SHPRD_HOST_URL__: host },
  });
}

afterEach(() => {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: originalWindow,
  });
});

describe("configured remote host", () => {
  test("uses validated host for API and WebSocket transport", () => {
    // Given: Android injects a reachable HTTPS Herdr origin.
    installHost("https://herdr.example/");

    // When: React resolves transport endpoints.
    const api = apiUrl("/api/health");
    const socket = bridgeWebSocketUrl({ protocol: "file:", host: "" });

    // Then: API and WebSocket traffic target the remote host.
    expect(api).toBe("https://herdr.example/api/health");
    expect(socket).toBe("wss://herdr.example/ws");
  });

  test("keeps browser relative transport without Android host config", () => {
    // Given: browser runtime has no Android configuration.
    installHost();

    // When: React resolves transport endpoints.
    const api = apiUrl("/api/health");
    const socket = bridgeWebSocketUrl({
      protocol: "https:",
      host: "shprd.test",
    });

    // Then: current same-origin behavior remains intact.
    expect(api).toBe("/api/health");
    expect(socket).toBe("wss://shprd.test/ws");
  });

  test("rejects host values outside the origin contract", () => {
    // Given: malformed Android configuration.
    installHost("https://user:pass@herdr.example/path?token=secret");

    // When: React resolves transport endpoints.
    const api = apiUrl("/api/health");
    const socket = bridgeWebSocketUrl({
      protocol: "http:",
      host: "shprd.test",
    });

    // Then: it never routes traffic to the malformed value.
    expect(api).toBe("/api/health");
    expect(socket).toBe("ws://shprd.test/ws");
  });
});
