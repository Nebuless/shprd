(async (dioxus) => {
  const { origin, request_id } = await dioxus.recv();
  const frame = document.getElementById("shprd-react");
  if (!frame || !frame.contentWindow)
    throw new Error("React surface unavailable");
  const source = frame.contentWindow;
  return await new Promise((resolve, reject) => {
    const cleanup = () => {
      clearTimeout(timeout);
      window.removeEventListener("message", receive);
      frame.removeEventListener("load", navigated);
    };
    const navigated = () => {
      cleanup();
      reject(new Error("React surface navigated"));
    };
    const receive = (event) => {
      if (event.source !== source || event.origin !== origin) return;
      const data = event.data;
      if (
        !data ||
        typeof data !== "object" ||
        Array.isArray(data) ||
        Object.keys(data).sort().join(",") !== "protocol,request_id,type" ||
        data.protocol !== "shprd.shell.v1" ||
        data.type !== "ack" ||
        data.request_id !== request_id
      )
        return;
      cleanup();
      resolve(data);
    };
    const timeout = setTimeout(() => {
      cleanup();
      reject(new Error("React bridge did not respond"));
    }, 5000);
    window.addEventListener("message", receive);
    frame.addEventListener("load", navigated);
    source.postMessage(
      { protocol: "shprd.shell.v1", type: "ping", request_id },
      origin,
    );
  });
})(dioxus);
