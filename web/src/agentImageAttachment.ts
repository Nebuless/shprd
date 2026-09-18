export const MAX_IMAGE_BYTES = 25 * 1024 * 1024;

const SUPPORTED_IMAGE_MIME_TYPES = new Set([
  "image/png",
  "image/jpeg",
  "image/gif",
  "image/webp",
  "image/bmp",
  "image/x-icon",
  "image/vnd.microsoft.icon",
  "image/avif",
]);

export type ImageAttachment = { mimeType: string; data: string };

export function isSupportedImageMimeType(mimeType: string): boolean {
  return SUPPORTED_IMAGE_MIME_TYPES.has(mimeType.toLowerCase());
}

export function getSafeImageSourceLabel(source: string): string {
  try {
    const parsed = new URL(source, "http://localhost");
    const label = decodeURIComponent(
      parsed.pathname.split("/").pop() ?? "",
    ).trim();
    return label || "image";
  } catch {
    const withoutQuery = source.split(/[?#]/, 1)[0];
    const label = withoutQuery.split(/[\\/]/).pop()?.trim();
    return label ? decodeURIComponent(label) : "image";
  }
}

export function readImageAttachment(blob: Blob): Promise<ImageAttachment> {
  const declaredMimeType = blob.type.toLowerCase();
  if (!isSupportedImageMimeType(declaredMimeType)) {
    return Promise.reject(new Error("Unsupported image type"));
  }
  if (blob.size > MAX_IMAGE_BYTES) {
    return Promise.reject(new Error("Image exceeds 25 MiB"));
  }

  const mimeType =
    declaredMimeType === "image/vnd.microsoft.icon"
      ? "image/x-icon"
      : declaredMimeType;
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== "string") {
        reject(new Error("Could not read image"));
        return;
      }
      const comma = result.indexOf(",");
      resolve({ mimeType, data: comma < 0 ? result : result.slice(comma + 1) });
    };
    reader.onerror = () => reject(new Error("Could not read image"));
    reader.readAsDataURL(blob);
  });
}

export function createEphemeralImageSlot() {
  let objectUrl: string | undefined;
  const revoke = () => {
    if (objectUrl === undefined) return;
    URL.revokeObjectURL(objectUrl);
    objectUrl = undefined;
  };

  return {
    set(nextUrl: string) {
      if (nextUrl !== objectUrl) revoke();
      objectUrl = nextUrl;
      return objectUrl;
    },
    remove() {
      revoke();
    },
    get current() {
      return objectUrl;
    },
  };
}
