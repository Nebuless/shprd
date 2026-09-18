import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { installShellBridge } from "../../../web/src/shellBridge.ts";

installShellBridge(window.shellOrigin);
function requestShell(type, fields = {}) {
  const request_id = `fixture-${crypto.randomUUID()}`;
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      window.removeEventListener("message", receive);
      reject(new Error("Shell control response missing"));
    }, 5000);
    function receive(event) {
      const data = event.data;
      if (
        event.source !== window.parent ||
        event.origin !== window.shellOrigin ||
        !data ||
        data.protocol !== "shprd.shell.v1" ||
        data.request_id !== request_id
      ) {
        return;
      }
      clearTimeout(timeout);
      window.removeEventListener("message", receive);
      resolve(data);
    }
    window.addEventListener("message", receive);
    window.parent.postMessage(
      { protocol: "shprd.shell.v1", type, request_id, ...fields },
      window.shellOrigin,
    );
  });
}
window.requestShellCheck = async () => {
  const { request_id: _requestId, ...reply } =
    await requestShell("bridge.check");
  return reply;
};
window.requestShellHost = (host) => requestShell("host.open", { host });
function DraftFixture() {
  const [draft, setDraft] = useState("");
  return (
    <main
      style={{
        fontFamily: "system-ui",
        padding: 24,
        color: "#d5dcff",
        background: "#1a1b26",
        minHeight: "100vh",
        boxSizing: "border-box",
      }}
    >
      <h1>React workspace bridge fixture</h1>
      <p>Controlled React draft. Shell does not own this document.</p>
      <label htmlFor="draft">Draft</label>
      <textarea
        id="draft"
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        style={{
          display: "block",
          boxSizing: "border-box",
          width: "100%",
          minHeight: 160,
          marginTop: 8,
        }}
      />
    </main>
  );
}
createRoot(document.getElementById("root")).render(<DraftFixture />);
