import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import type { ConnectionClient } from "./api";
import {
  imageUploadUrl,
  uploadTerminalImage,
  uploadTerminalImageToPane,
} from "./terminalImageUpload";

const originalFetch = globalThis.fetch;
const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
const client = {
  connectionId: "conn-a",
  generation: 1,
  serverRuntimeGeneration: 3,
  call: async () => undefined,
  isCurrent: () => true,
  acceptsServerGeneration: () => true,
} satisfies ConnectionClient;
const image = new File(["image"], "image.png", { type: "image/png" });

beforeEach(() => {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { location: { origin: "https://studio.example" } },
  });
});

afterEach(() => {
  globalThis.fetch = originalFetch;
  if (originalWindow) {
    Object.defineProperty(globalThis, "window", originalWindow);
  } else {
    delete (globalThis as { window?: unknown }).window;
  }
});

describe("terminal image upload responses", () => {
  test("recognizes only HTTP(S) image URLs", () => {
    expect(imageUploadUrl(" https://images.example/image.png ")).toBe(
      "https://images.example/image.png",
    );
    expect(imageUploadUrl("file:///tmp/image.png")).toBeNull();
  });

  test("returns a validated path", async () => {
    globalThis.fetch = (async () =>
      Response.json({ path: "/tmp/image.png" })) as unknown as typeof fetch;

    await expect(uploadTerminalImage(client, image)).resolves.toBe(
      "/tmp/image.png",
    );
  });

  test("asks the connection endpoint to retrieve an image URL", async () => {
    const requests: Array<{ headers?: HeadersInit }> = [];
    globalThis.fetch = (async (
      _input: RequestInfo | URL,
      init?: RequestInit,
    ) => {
      requests.push(init ?? {});
      return Response.json({ path: "/tmp/image.png" });
    }) as unknown as typeof fetch;

    await expect(
      uploadTerminalImage(client, "https://images.example/image.png"),
    ).resolves.toBe("/tmp/image.png");

    expect(requests).toHaveLength(1);
    expect(new Headers(requests[0]?.headers).get("x-image-url")).toBe(
      "https://images.example/image.png",
    );
  });

  test("uploads URL then sends returned path to requested pane", async () => {
    const calls: Array<{ method: string; params: unknown }> = [];
    const paneClient = {
      ...client,
      call: async (method: string, params: unknown) => {
        calls.push({ method, params });
      },
    } as ConnectionClient;
    globalThis.fetch = (async () =>
      Response.json({
        path: "/tmp/herdr-img-upload.png",
      })) as unknown as typeof fetch;

    await uploadTerminalImageToPane(
      paneClient,
      "https://images.example/image.png",
      "active-pane",
    );

    expect(calls).toEqual([
      {
        method: "pane.send_input",
        params: {
          pane_id: "active-pane",
          text: "/tmp/herdr-img-upload.png",
          keys: [],
        },
      },
    ]);
  });

  test("rejects malformed JSON instead of silently continuing", async () => {
    globalThis.fetch = (async () =>
      new Response("{", {
        status: 200,
        headers: { "content-type": "application/json" },
      })) as unknown as typeof fetch;

    await expect(uploadTerminalImage(client, image)).rejects.toThrow();
  });

  test("surfaces non-JSON HTTP error bodies", async () => {
    globalThis.fetch = (async () =>
      new Response("proxy authentication required", {
        status: 407,
        statusText: "Proxy Authentication Required",
      })) as unknown as typeof fetch;

    await expect(uploadTerminalImage(client, image)).rejects.toThrow(
      "proxy authentication required",
    );
  });

  test("uses structured errors from failed JSON responses", async () => {
    globalThis.fetch = (async () =>
      Response.json(
        { error: "image is too large" },
        { status: 413, statusText: "Payload Too Large" },
      )) as unknown as typeof fetch;

    await expect(uploadTerminalImage(client, image)).rejects.toThrow(
      "image is too large",
    );
  });

  test("rejects successful responses without a path", async () => {
    globalThis.fetch = (async () =>
      Response.json({})) as unknown as typeof fetch;

    await expect(uploadTerminalImage(client, image)).rejects.toThrow(
      "image upload response did not include a path",
    );
  });
});
