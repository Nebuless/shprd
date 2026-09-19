import { describe, expect, test } from "bun:test";
import { createImageUploadHandler } from "./image-upload";

describe("image upload URLs", () => {
  test("rejects image URLs without HTTP or HTTPS", async () => {
    const handler = createImageUploadHandler({ sshHost: () => undefined });
    const response = await handler(
      new Request("http://studio.test/upload-image", {
        method: "POST",
        headers: { "x-image-url": "file:///tmp/image.png" },
      }),
    );

    expect(response.status).toBe(400);
    expect(await response.json()).toEqual({
      error: "image URL must use HTTP or HTTPS",
    });
  });

  test("rejects image URLs resolving to private hosts", async () => {
    const handler = createImageUploadHandler({
      sshHost: () => undefined,
      resolveHost: async () => ["127.0.0.1"],
    });
    const response = await handler(
      new Request("http://studio.test/upload-image", {
        method: "POST",
        headers: { "x-image-url": "https://images.example/image.png" },
      }),
    );

    expect(response.status).toBe(400);
    expect(await response.json()).toEqual({
      error: "image URL must resolve to a public address",
    });
  });

  test("downloads an HTTP image then writes it on the Herdr host", async () => {
    const requestedUrls: string[] = [];
    const writes: Array<{ path: string; data: Uint8Array }> = [];
    const handler = createImageUploadHandler({
      sshHost: () => undefined,
      resolveHost: async () => ["203.0.113.10"],
      fetchImage: async (url) => {
        requestedUrls.push(url.toString());
        return new Response(new Uint8Array([1, 2, 3]), {
          headers: { "content-type": "image/webp" },
        });
      },
      writeFile: async (path, data) => {
        writes.push({ path, data });
      },
    });

    const response = await handler(
      new Request("http://studio.test/upload-image", {
        method: "POST",
        headers: {
          "x-image-url": "https://images.example/diagram.webp",
        },
      }),
    );

    expect(response.status).toBe(200);
    expect(requestedUrls).toEqual(["https://images.example/diagram.webp"]);
    expect(writes).toHaveLength(1);
    expect(writes[0]?.path).toEndWith(".webp");
    expect(writes[0]?.data).toEqual(new Uint8Array([1, 2, 3]));
    expect(await response.json()).toMatchObject({
      path: expect.stringMatching(/^\/tmp\/herdr-img-/),
      remote: false,
    });
  });
});
