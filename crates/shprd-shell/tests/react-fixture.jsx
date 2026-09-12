import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { installShellBridge } from "../assets/react-bridge.mjs";

installShellBridge(window.shellOrigin);
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
