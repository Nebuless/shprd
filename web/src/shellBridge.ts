const bridgeProtocol = "shprd.shell.v1";
const requestId = /^[A-Za-z0-9-]{1,64}$/;

type BridgePacket = {
  protocol: string;
  type: string;
  request_id: string;
};

type ShellReply =
  | {
      protocol: typeof bridgeProtocol;
      type: "bridge.checked" | "bridge.failed" | "host.failed";
      request_id: string;
    }
  | {
      protocol: typeof bridgeProtocol;
      type: "host.opened";
      request_id: string;
      host: string;
    };

function isBridgePing(value: unknown): value is BridgePacket {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const data = value as Record<string, unknown>;
  return (
    Object.keys(data).sort().join(",") === "protocol,request_id,type" &&
    data.protocol === bridgeProtocol &&
    data.type === "ping" &&
    typeof data.request_id === "string" &&
    requestId.test(data.request_id)
  );
}

function isShellReply(value: unknown, requestId: string): value is ShellReply {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const data = value as Record<string, unknown>;
  if (
    data.protocol !== bridgeProtocol ||
    data.request_id !== requestId ||
    typeof data.type !== "string"
  ) {
    return false;
  }
  if (data.type === "host.opened") {
    return (
      Object.keys(data).sort().join(",") === "host,protocol,request_id,type" &&
      typeof data.host === "string"
    );
  }
  return (
    ["bridge.checked", "bridge.failed", "host.failed"].includes(data.type) &&
    Object.keys(data).sort().join(",") === "protocol,request_id,type"
  );
}

function shellOrigin() {
  if (window.parent === window || !document.referrer) return null;
  try {
    const url = new URL(document.referrer);
    return ["http:", "https:"].includes(url.protocol) ? url.origin : null;
  } catch {
    return null;
  }
}

function normalizedShellHost(value: string) {
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
    return url.href;
  } catch {
    return null;
  }
}

export function installShellBridge(parentOrigin: string, owner = window) {
  const origin = new URL(parentOrigin);
  if (
    !["http:", "https:"].includes(origin.protocol) ||
    origin.origin !== parentOrigin
  ) {
    throw new TypeError("Expected an exact HTTP(S) parent origin");
  }
  const receive = (event: MessageEvent<unknown>) => {
    if (
      owner.parent === owner ||
      event.source !== owner.parent ||
      event.origin !== parentOrigin ||
      !isBridgePing(event.data)
    ) {
      return;
    }
    owner.parent.postMessage(
      {
        protocol: bridgeProtocol,
        type: "ack",
        request_id: event.data.request_id,
      },
      parentOrigin,
    );
  };
  owner.addEventListener("message", receive);
  return () => owner.removeEventListener("message", receive);
}

export function installShellBridgeFromReferrer() {
  const parentOrigin = shellOrigin();
  if (parentOrigin) installShellBridge(parentOrigin);
}

export function shellHostUrl() {
  return shellOrigin() ? `${window.location.origin}/` : null;
}

export function shellHostChangesCurrent(value: string) {
  return normalizedShellHost(value) !== shellHostUrl();
}

export function requestShellControl(
  type: "bridge.check" | "host.open",
  host?: string,
): Promise<ShellReply> {
  const parentOrigin = shellOrigin();
  if (!parentOrigin)
    return Promise.reject(new Error("Shell controls unavailable"));
  const request_id = `react-${crypto.randomUUID()}`;
  const message =
    type === "host.open"
      ? { protocol: bridgeProtocol, type, request_id, host: host ?? "" }
      : { protocol: bridgeProtocol, type, request_id };
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      window.removeEventListener("message", receive);
      reject(new Error("Shell did not respond"));
    }, 5000);
    const receive = (event: MessageEvent<unknown>) => {
      const value = event.data;
      if (
        event.source !== window.parent ||
        event.origin !== parentOrigin ||
        !value ||
        typeof value !== "object" ||
        Array.isArray(value)
      ) {
        return;
      }
      if (!isShellReply(value, request_id)) return;
      window.clearTimeout(timeout);
      window.removeEventListener("message", receive);
      resolve(value);
    };
    window.addEventListener("message", receive);
    window.parent.postMessage(message, parentOrigin);
  });
}
