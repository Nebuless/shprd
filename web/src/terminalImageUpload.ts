import type { ConnectionClient } from "./api";
import { connectionHttpPath } from "./connectionHttp";
import { apiUrl } from "./remoteHost";
import { terminalPasteRequest } from "./terminalPaste";

export function imageUploadUrl(value: string): string | null {
  try {
    const url = new URL(value.trim());
    return ["http:", "https:"].includes(url.protocol) ? url.toString() : null;
  } catch {
    return null;
  }
}

export async function readTerminalClipboardImage(
  clipboard: Pick<Clipboard, "read"> | undefined,
): Promise<Blob> {
  if (!clipboard?.read) {
    throw new Error(
      "Clipboard access is unavailable. Paste in the terminal or attach a file.",
    );
  }
  const items = await clipboard.read();
  for (const item of items) {
    const imageType = item.types.find((type) => type.startsWith("image/"));
    if (imageType) return item.getType(imageType);
  }
  throw new Error("No image in clipboard. Copy an image or attach a file.");
}

/**
 * Uploads an image through the connection's HTTP endpoint and returns the
 * server-side path. Callers decide when (or whether) that path reaches a
 * terminal; uploading here never writes to a PTY by itself.
 */
export async function uploadTerminalImage(
  client: ConnectionClient,
  image: File | string,
): Promise<string> {
  if (!client.isCurrent()) throw new Error("connection changed during upload");
  const imageUrl = typeof image === "string" ? imageUploadUrl(image) : null;
  if (typeof image === "string" && !imageUrl) {
    throw new Error("image URL must use HTTP or HTTPS");
  }
  const file = typeof image === "string" ? null : image;
  const ext = file ? (file.type.split("/")[1] || "png").toLowerCase() : "png";
  const requestPath = connectionHttpPath(
    client.connectionId,
    "/upload-image",
    client.serverRuntimeGeneration,
  );
  const remoteOrigin = apiUrl("/");
  const uploadUrl = new URL(
    requestPath,
    remoteOrigin.startsWith("/") ? window.location.origin : remoteOrigin,
  );
  if (
    uploadUrl.origin !== new URL(remoteOrigin, window.location.origin).origin
  ) {
    throw new Error("invalid upload origin");
  }
  const res = await fetch(uploadUrl, {
    method: "POST",
    headers: file
      ? {
          "x-image-ext": ext,
          "content-type": file.type || "image/png",
        }
      : { "x-image-url": imageUrl ?? "" },
    body: file,
  });
  if (!res.ok) {
    const body = (await res.text()).trim();
    if (!client.isCurrent()) {
      throw new Error("connection changed during upload");
    }
    let detail = body;
    if (body) {
      try {
        const payload: unknown = JSON.parse(body);
        if (payload && typeof payload === "object" && !Array.isArray(payload)) {
          const error = (payload as { error?: unknown }).error;
          if (typeof error === "string" && error) detail = error;
        }
      } catch {
        // Auth proxies and generic HTTP servers commonly return plain text or
        // HTML errors. The response is already a failure, so preserve its body.
      }
    }
    throw new Error(
      detail || res.statusText || `Image upload failed (${res.status})`,
    );
  }

  const data: unknown = await res.json();
  if (!client.isCurrent()) throw new Error("connection changed during upload");
  if (data === null || typeof data !== "object" || Array.isArray(data)) {
    throw new Error("image upload response was not an object");
  }
  const payload = data as { path?: unknown };
  if (typeof payload.path !== "string" || payload.path.length === 0) {
    throw new Error("image upload response did not include a path");
  }
  return payload.path;
}

export async function uploadTerminalImageToPane(
  client: ConnectionClient,
  image: File | string,
  paneId: string,
): Promise<void> {
  const path = await uploadTerminalImage(client, image);
  const request = terminalPasteRequest(paneId, path);
  await client.call(request.method, request.params);
}
