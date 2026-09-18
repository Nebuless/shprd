(() => {
  const protocol = "shprd.shell.v1";
  const requestId = /^[A-Za-z0-9-]{1,64}$/;
  const frame = () => document.getElementById("shprd-react");
  const record = (value) =>
    value && typeof value === "object" && !Array.isArray(value) ? value : null;
  const exact = (value, keys) =>
    Object.keys(value).sort().join(",") === [...keys].sort().join(",");
  const current = (event, data) => {
    const surface = frame();
    if (!surface || event.source !== surface.contentWindow) return null;
    const origin = new URL(surface.src).origin;
    return event.origin === origin && data.protocol === protocol
      ? origin
      : null;
  };
  const host = (value) => {
    if (typeof value !== "string" || /\s/.test(value)) return null;
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
  };
  const reply = (source, origin, type, request_id, extra = {}) =>
    source.postMessage({ protocol, type, request_id, ...extra }, origin);

  window.addEventListener("message", (event) => {
    const data = record(event.data);
    if (
      !data ||
      typeof data.type !== "string" ||
      typeof data.request_id !== "string"
    ) {
      return;
    }
    const origin = current(event, data);
    if (!origin || !requestId.test(data.request_id)) return;

    if (
      data.type === "bridge.check" &&
      exact(data, ["protocol", "request_id", "type"])
    ) {
      const ping = `shell-${crypto.randomUUID()}`;
      const receive = (ack) => {
        const value = record(ack.data);
        if (
          ack.source !== event.source ||
          ack.origin !== origin ||
          !value ||
          !exact(value, ["protocol", "request_id", "type"]) ||
          value.protocol !== protocol ||
          value.type !== "ack" ||
          value.request_id !== ping
        ) {
          return;
        }
        clearTimeout(timeout);
        window.removeEventListener("message", receive);
        reply(event.source, origin, "bridge.checked", data.request_id);
      };
      const timeout = window.setTimeout(() => {
        window.removeEventListener("message", receive);
        reply(event.source, origin, "bridge.failed", data.request_id);
      }, 5000);
      window.addEventListener("message", receive);
      event.source.postMessage(
        { protocol, type: "ping", request_id: ping },
        origin,
      );
      return;
    }

    if (
      data.type === "host.open" &&
      exact(data, ["host", "protocol", "request_id", "type"])
    ) {
      const next = host(data.host);
      if (!next) {
        reply(event.source, origin, "host.failed", data.request_id);
        return;
      }
      reply(event.source, origin, "host.opened", data.request_id, {
        host: next,
      });
      const surface = frame();
      if (surface && surface.src !== next)
        window.setTimeout(() => (surface.src = next));
    }
  });
})();
