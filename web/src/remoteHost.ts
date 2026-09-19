type HostLocation = Pick<Location, "host" | "protocol">;

declare global {
  interface Window {
    __SHPRD_HOST_URL__?: string;
  }
}

function configuredHost(value: unknown): URL | null {
  if (typeof value !== "string" || value.length === 0 || /\s/.test(value)) {
    return null;
  }
  try {
    const url = new URL(value);
    if (
      !["http:", "https:"].includes(url.protocol) ||
      !url.hostname ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      url.pathname !== "/"
    ) {
      return null;
    }
    return url;
  } catch {
    return null;
  }
}

function configuredOrigin(): string | null {
  const configured =
    typeof window === "undefined" ? undefined : window.__SHPRD_HOST_URL__;
  return configuredHost(configured)?.origin ?? null;
}

export function apiUrl(path: string): string {
  const origin = configuredOrigin();
  return origin ? new URL(path, origin).href : path;
}

export function bridgeWebSocketUrl(
  location: HostLocation = globalThis.location,
): string {
  const origin = configuredOrigin();
  if (origin) {
    const url = new URL(origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    url.pathname = "/ws";
    return url.href;
  }
  const protocol = location.protocol === "https:" ? "wss" : "ws";
  return `${protocol}://${location.host}/ws`;
}
