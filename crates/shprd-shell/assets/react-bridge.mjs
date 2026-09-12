// Install in the retained React entry point, never in a shared Dioxus DOM root.
export function installShellBridge(parentOrigin, owner = window) {
  const origin = new URL(parentOrigin);
  if (
    !["http:", "https:"].includes(origin.protocol) ||
    origin.origin !== parentOrigin
  ) {
    throw new TypeError("Expected an exact HTTP(S) parent origin");
  }
  const receive = (event) => {
    if (
      owner.parent === owner ||
      event.source !== owner.parent ||
      event.origin !== parentOrigin
    )
      return;
    const data = event.data;
    if (
      !data ||
      typeof data !== "object" ||
      Array.isArray(data) ||
      Object.keys(data).sort().join(",") !== "protocol,request_id,type" ||
      data.protocol !== "shprd.shell.v1" ||
      data.type !== "ping" ||
      typeof data.request_id !== "string" ||
      !/^[A-Za-z0-9-]{1,64}$/.test(data.request_id)
    )
      return;
    owner.parent.postMessage(
      { protocol: "shprd.shell.v1", type: "ack", request_id: data.request_id },
      parentOrigin,
    );
  };
  owner.addEventListener("message", receive);
  return () => owner.removeEventListener("message", receive);
}
