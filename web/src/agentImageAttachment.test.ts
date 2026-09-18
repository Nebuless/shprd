import { afterEach, expect, mock, test } from "bun:test";
import {
  MAX_IMAGE_BYTES,
  createEphemeralImageSlot,
  getSafeImageSourceLabel,
  isSupportedImageMimeType,
  readImageAttachment,
} from "./agentImageAttachment";

const originalFileReader = globalThis.FileReader;

afterEach(() => {
  globalThis.FileReader = originalFileReader;
});

test("accepts supported raster MIME types and rejects other types", () => {
  for (const mimeType of [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/x-icon",
    "image/vnd.microsoft.icon",
    "image/avif",
  ]) {
    expect(isSupportedImageMimeType(mimeType)).toBe(true);
  }
  expect(isSupportedImageMimeType("image/svg+xml")).toBe(false);
  expect(isSupportedImageMimeType("IMAGE/PNG")).toBe(true);
});

test("keeps source labels to safe final path components", () => {
  expect(
    getSafeImageSourceLabel("/home/user/private/photo.png?token=secret#view"),
  ).toBe("photo.png");
  expect(
    getSafeImageSourceLabel(
      "https://example.test/images/photo%20one.jpg?token=secret",
    ),
  ).toBe("photo one.jpg");
  expect(getSafeImageSourceLabel("https://example.test/")).toBe("image");
});

test("converts blob to base64 without data URL prefix", async () => {
  class MockFileReader {
    result: string | ArrayBuffer | null = null;
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;
    readAsDataURL() {
      this.result = "data:image/png;base64,aGVsbG8=";
      this.onload?.();
    }
  }
  globalThis.FileReader = MockFileReader as unknown as typeof FileReader;

  const result = await readImageAttachment(
    new Blob(["hello"], { type: "image/png" }),
  );
  expect(result).toEqual({ mimeType: "image/png", data: "aGVsbG8=" });
});

test("normalizes vendor ICO MIME for native image prompts", async () => {
  class MockFileReader {
    result: string | ArrayBuffer | null = null;
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;
    readAsDataURL() {
      this.result = "data:image/vnd.microsoft.icon;base64,aWNv";
      this.onload?.();
    }
  }
  globalThis.FileReader = MockFileReader as unknown as typeof FileReader;

  expect(
    await readImageAttachment(
      new Blob(["ico"], { type: "image/vnd.microsoft.icon" }),
    ),
  ).toEqual({ mimeType: "image/x-icon", data: "aWNv" });
});

test("rejects unsupported MIME and oversized blobs", async () => {
  await expect(
    readImageAttachment(new Blob(["x"], { type: "image/svg+xml" })),
  ).rejects.toThrow("Unsupported image type");
  const blob = { type: "image/png", size: MAX_IMAGE_BYTES + 1 } as Blob;
  await expect(readImageAttachment(blob)).rejects.toThrow(
    "Image exceeds 25 MiB",
  );
});

test("revokes each object URL once on replacement and removal", () => {
  const revoke = mock(() => {});
  const originalUrl = globalThis.URL;
  globalThis.URL = {
    ...originalUrl,
    revokeObjectURL: revoke,
  } as unknown as typeof URL;
  const slot = createEphemeralImageSlot();

  slot.set("blob:first");
  slot.set("blob:second");
  slot.set("blob:second");
  slot.remove();
  slot.remove();

  expect(revoke).toHaveBeenCalledTimes(2);
  expect(revoke).toHaveBeenNthCalledWith(1, "blob:first");
  expect(revoke).toHaveBeenNthCalledWith(2, "blob:second");
  globalThis.URL = originalUrl;
});
