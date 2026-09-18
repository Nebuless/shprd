import { expect, test } from "bun:test";
import {
  AgentWorkspaceClient,
  createAgentTransport,
  imageFetchPath,
  parseAgentSessions,
  parseAgentMessages,
  parseAgentModels,
  record,
  type AgentSession,
  type AgentTransport,
} from "./agentClient";
import type { ConnectionStatus } from "./api";

test("native subscription precedes snapshots and loss disables writes", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    const subscription = f.calls.findIndex(
      (call) => call.method === "agent_control.subscribe",
    );
    const snapshot = f.calls.findIndex(
      (call) => record(call.params?.command).type === "get_messages",
    );
    expect(subscription).toBeGreaterThanOrEqual(0);
    expect(subscription).toBeLessThan(snapshot);
    f.client.setDraft("keep draft");
    f.event({
      agent_event: { session_id: "s1", event: { type: "attachment_lost" } },
    });
    expect(f.client.getSnapshot().sessions[0].connected).toBe(false);
    const count = f.calls.length;
    await f.client.command({ type: "prompt", message: "keep draft" });
    expect(f.calls.length).toBe(count);
    expect(f.client.getSnapshot().drafts.s1).toBe("keep draft");
  } finally {
    f.stop();
  }
});

test("manual refresh resubscribes after native loss without replaying prompt", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    f.client.setDraft("retained");
    f.event({
      agent_event: { session_id: "s1", event: { type: "attachment_lost" } },
    });
    await f.client.refresh();
    expect(
      f.calls.filter((call) => call.method === "agent_control.subscribe")
        .length,
    ).toBe(2);
    f.event({
      agent_event: { session_id: "s1", event: { type: "agent_start" } },
    });
    expect(f.client.getSnapshot().sessions[0].busy).toBe(true);
    expect(f.client.getSnapshot().drafts.s1).toBe("retained");
    expect(
      f.calls.some((call) => record(call.params?.command).type === "prompt"),
    ).toBe(false);
  } finally {
    f.stop();
  }
});

test("native UI dialog events request terminal handoff", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    f.event({
      agent_event: {
        session_id: "s1",
        event: { type: "ui_dialog_request", response_owner: "native_ui" },
      },
    });
    expect(Boolean(f.client.getSnapshot().error)).toBe(true);
  } finally {
    f.stop();
  }
});

test("builds scoped encoded native image fetch paths", () => {
  expect(
    imageFetchPath(
      "native:one",
      7,
      "https://images.example.test/photo.png?size=large",
    ),
  ).toBe(
    "/api/connections/native%3Aone/image-fetch?connection_generation=7&url=https%3A%2F%2Fimages.example.test%2Fphoto.png%3Fsize%3Dlarge",
  );
});

test("defaults URL image fetch support off until native host advertises it", () => {
  const client = {
    connectionId: "local",
    generation: 1,
    serverRuntimeGeneration: 1,
    call: async () => ({}),
    isCurrent: () => true,
    acceptsServerGeneration: () => true,
  };
  expect(
    createAgentTransport(
      client,
      () => () => {},
      () => () => {},
    ).canFetchImageUrl,
  ).toBe(false);
  expect(
    createAgentTransport(
      client,
      () => () => {},
      () => () => {},
      true,
    ).canFetchImageUrl,
  ).toBe(true);
});

test("rejects image prompt when agent lacks image capability", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    await f.client.command({
      type: "prompt",
      message: "describe image",
      image: { mimeType: "image/png", data: "aGVsbG8=" },
    });
    expect(
      f.calls.some((call) => record(call.params?.command).type === "prompt"),
    ).toBe(false);
    expect(f.client.getSnapshot().error).toBe(
      "Image prompts are unavailable for this agent.",
    );
  } finally {
    f.stop();
  }
});

test("accepts native adapter model identifiers", () => {
  expect(
    parseAgentModels({
      models: [{ provider: "fixture", modelId: "free", name: "Free" }],
    }),
  ).toEqual([{ provider: "fixture", id: "free", name: "Free" }]);
});

test("abort acknowledgement keeps busy until native settlement", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    f.event({
      agent_event: { session_id: "s1", event: { type: "agent_start" } },
    });
    await f.client.command({ type: "abort" });
    expect(f.client.getSnapshot().sessions[0].busy).toBe(true);
    f.event({
      agent_event: { session_id: "s1", event: { type: "agent_settled" } },
    });
    expect(f.client.getSnapshot().sessions[0].busy).toBe(false);
  } finally {
    f.stop();
  }
});

test("preserves real session identity and connection state for both engines", () => {
  // Given
  const sessions: AgentSession[] = [
    {
      id: "s1",
      agent: "senpi",
      name: "Review",
      cwd: "/repo",
      connected: true,
      busy: false,
      capabilities: { image_prompt: false },
    },
    {
      id: "a1",
      agent: "atomic",
      name: "Build",
      cwd: "/repo",
      connected: false,
      busy: true,
      capabilities: { image_prompt: true },
    },
  ];
  // When
  const result = parseAgentSessions({ sessions });
  // Then
  expect(result).toEqual(sessions);
});

const sessions = [
  {
    id: "s1",
    agent: "senpi",
    name: "Review",
    cwd: "/repo",
    connected: true,
    busy: false,
    capabilities: { image_prompt: false },
  },
  {
    id: "a1",
    agent: "atomic",
    name: "Build",
    cwd: "/repo",
    connected: true,
    busy: false,
    capabilities: { image_prompt: true },
  },
];
function fixture() {
  let status: (value: ConnectionStatus) => void = () => {};
  let event: (value: unknown) => void = () => {};
  let unsubscribed = 0;
  const calls: { method: string; params?: Record<string, unknown> }[] = [];
  const transport: AgentTransport = {
    scopeKey: "local",
    canFetchImageUrl: false,
    fetchImage: async () => new Blob(["image"], { type: "image/png" }),
    call: async (method, params) => {
      calls.push({ method, params });
      if (method === "agent_control.list") return { sessions };
      const command = record(params?.command);
      switch (command.type) {
        case "get_messages":
          return {
            type: "response",
            success: true,
            data: { messages: [{ role: "user", content: params?.session_id }] },
          };
        case "get_state":
          return {
            type: "response",
            success: true,
            data: {
              isStreaming: false,
              model: { provider: "test", id: "model" },
            },
          };
        case "get_available_models":
          return {
            type: "response",
            success: true,
            data: { models: [{ provider: "test", id: "model" }] },
          };
        default:
          return { type: "response", success: true };
      }
    },
    subscribe: (listener) => {
      event = listener;
      return () => {
        unsubscribed++;
        event = () => {};
      };
    },
    onStatus: (listener) => {
      status = listener;
      return () => {
        unsubscribed++;
        status = () => {};
      };
    },
  };
  const client = new AgentWorkspaceClient(transport);
  const stop = client.start();
  status("connected");
  return {
    client,
    stop,
    calls,
    transport,
    status: (s: ConnectionStatus) => status(s),
    event: (e: unknown) => event(e),
    unsubscribed: () => unsubscribed,
  };
}
function signal(client: AgentWorkspaceClient, condition: () => boolean) {
  if (condition()) return Promise.resolve();
  return new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      off();
      reject(new Error("State signal timed out"));
    }, 2000);
    const off = client.subscribe(() => {
      if (condition()) {
        clearTimeout(timeout);
        off();
        resolve();
      }
    });
  });
}

test("defaults missing image capability to unsupported", () => {
  expect(
    parseAgentSessions({
      sessions: [
        {
          id: "s1",
          agent: "senpi",
          name: "Review",
          cwd: "/repo",
          connected: true,
          busy: false,
        },
      ],
    }),
  ).toEqual([
    expect.objectContaining({ capabilities: { image_prompt: false } }),
  ]);
});

test("rejects malformed sessions and native command errors", () => {
  expect(() =>
    parseAgentSessions({ sessions: [{ ...sessions[0], agent: "other" }] }),
  ).toThrow();
  expect(() =>
    parseAgentSessions({ sessions: [sessions[0], sessions[0]] }),
  ).toThrow();
  expect(() =>
    parseAgentMessages({
      type: "response",
      success: false,
      error: "not allowed",
    }),
  ).toThrow("not allowed");
  expect(() =>
    parseAgentModels({ models: [{ id: "missing-provider" }] }),
  ).toThrow();
});

test("switches sessions without losing drafts and releases subscriptions", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    f.client.setDraft("keep draft");
    await f.client.select("a1");
    f.client.setDraft("atomic draft");
    await f.client.select("s1");
    expect(f.client.getSnapshot().drafts).toEqual({
      s1: "keep draft",
      a1: "atomic draft",
    });
    expect(f.client.getSnapshot().messages[0].content).toBe("s1");
  } finally {
    f.stop();
  }
  expect(f.unsubscribed()).toBe(2);
});

test("reconnect reloads authoritative state and retains offline draft", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    f.status("disconnected");
    f.client.setDraft("offline draft");
    const ready = signal(
      f.client,
      () =>
        f.client.getSnapshot().connected &&
        !f.client.getSnapshot().loading &&
        f.client.getSnapshot().messages.length === 1,
    );
    f.status("connected");
    await ready;
    expect(f.client.getSnapshot().drafts.s1).toBe("offline draft");
    expect(
      f.calls.filter((c) => c.method === "agent_control.list").length,
    ).toBe(2);
  } finally {
    f.stop();
  }
});

test("native message streaming replaces partial content and updates busy", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    for (const event of [
      { type: "agent_start" },
      {
        type: "message_start",
        message: {
          role: "assistant",
          content: [{ type: "text", text: "Hel" }],
        },
      },
      {
        type: "message_update",
        message: {
          role: "assistant",
          content: [{ type: "text", text: "Hello" }],
        },
      },
      {
        type: "message_end",
        message: {
          role: "assistant",
          content: [{ type: "text", text: "Hello!" }],
        },
      },
      { type: "agent_end" },
    ])
      f.event({ agent_event: { session_id: "s1", event } });
    expect(f.client.getSnapshot().messages).toHaveLength(2);
    expect(f.client.getSnapshot().messages[1].content).toEqual([
      { type: "text", text: "Hello!" },
    ]);
    expect(f.client.getSnapshot().sessions[0].busy).toBe(false);
  } finally {
    f.stop();
  }
});

test("rejects image prompt before unsupported native dispatch", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    f.client.setDraft("keep image draft");
    const callsBefore = f.calls.length;
    await f.client.command({
      type: "prompt",
      message: "keep image draft",
      image: { mimeType: "image/png", data: "iVBORw0KGgo=" },
    });
    expect(f.calls.length).toBe(callsBefore);
    expect(f.client.getSnapshot().drafts.s1).toBe("keep image draft");
    expect(f.client.getSnapshot().error).toBe(
      "Image prompts are unavailable for this agent.",
    );
  } finally {
    f.stop();
  }
});

test("sends image prompt to capable current session", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    await f.client.select("a1");
    f.client.setDraft("inspect image");
    const image = { mimeType: "image/png", data: "iVBORw0KGgo=" };
    expect(
      await f.client.command({
        type: "prompt",
        message: "inspect image",
        image,
      }),
    ).toBe(true);
    expect(f.calls[f.calls.length - 1]).toEqual({
      method: "agent_control.request",
      params: {
        session_id: "a1",
        command: { type: "prompt", message: "inspect image", image },
      },
    });
  } finally {
    f.stop();
  }
});

test("failed prompt preserves draft and native error", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    f.client.setDraft("retry this");
    f.transport.call = async () => ({
      type: "response",
      success: false,
      error: "model offline",
    });
    await f.client.command({ type: "prompt", message: "retry this" });
    expect(f.client.getSnapshot().drafts.s1).toBe("retry this");
    expect(f.client.getSnapshot().error).toBe("model offline");
  } finally {
    f.stop();
  }
});

test("late selected-session response cannot overwrite newer selection", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    const base = f.transport.call;
    let release = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    let entered = () => {};
    const started = new Promise<void>((resolve) => {
      entered = resolve;
    });
    f.transport.call = async (method, params) => {
      if (
        params?.session_id === "s1" &&
        record(params.command).type === "get_messages"
      ) {
        entered();
        await gate;
      }
      return base(method, params);
    };
    const old = f.client.select("s1");
    await started;
    await f.client.select("a1");
    release();
    await old;
    expect(f.client.getSnapshot().selectedId).toBe("a1");
    expect(f.client.getSnapshot().messages[0].content).toBe("a1");
  } finally {
    f.stop();
  }
});

test("transport filters connection and runtime generations", () => {
  let event: (value: unknown) => void = () => {};
  const accepted: unknown[] = [];
  const transport = createAgentTransport(
    {
      connectionId: "local",
      generation: 1,
      serverRuntimeGeneration: 2,
      call: async () => ({}),
      isCurrent: () => true,
      acceptsServerGeneration: (n) => n === 2,
    },
    (listener) => {
      event = listener;
      return () => {};
    },
    () => () => {},
  );
  transport.subscribe((value) => accepted.push(value));
  event({ connection_id: "other", connection_generation: 2 });
  event({ connection_id: "local", connection_generation: 1 });
  event({ connection_id: "local", connection_generation: 2 });
  expect(accepted).toEqual([
    { connection_id: "local", connection_generation: 2 },
  ]);
});

test("events arriving during initial load preserve history and latest streamed content", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    const base = f.transport.call;
    let release = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    f.transport.call = async (method, params) => {
      await gate;
      return base(method, params);
    };
    const loading = f.client.select("s1");
    f.event({
      agent_event: {
        session_id: "s1",
        event: {
          type: "message_start",
          message: { role: "assistant", content: "streamed" },
        },
      },
    });
    release();
    await loading;
    expect(f.client.getSnapshot().messages.map((m) => m.content)).toEqual([
      "s1",
      "streamed",
    ]);
  } finally {
    f.stop();
  }
});

test("removed selection clears messages without discarding draft", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    f.client.setDraft("saved");
    f.transport.call = async () => ({ sessions: [] });
    await f.client.refresh();
    expect(f.client.getSnapshot().selectedId).toBeNull();
    expect(f.client.getSnapshot().messages).toEqual([]);
    expect(f.client.getSnapshot().drafts.s1).toBe("saved");
  } finally {
    f.stop();
  }
});

test("sends native model and prompt commands and clears only acknowledged draft", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    await f.client.command({
      type: "set_model",
      provider: "test",
      modelId: "fast",
    });
    expect(f.calls[f.calls.length - 1]?.params).toEqual({
      session_id: "s1",
      command: { type: "set_model", provider: "test", modelId: "fast" },
    });
    f.client.setDraft("go");
    await f.client.command({ type: "prompt", message: "go" });
    expect(f.client.getSnapshot().drafts.s1).toBe("");
    expect(f.client.getSnapshot().sessions[0].busy).toBe(true);
  } finally {
    f.stop();
  }
});

test("abort cannot be undone by a late prompt acknowledgement", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    const base = f.transport.call;
    let release = () => {};
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    f.transport.call = async (method, params) => {
      if (record(params?.command).type === "prompt") await gate;
      return base(method, params);
    };
    const pending = f.client.command({ type: "prompt", message: "go" });
    await f.client.command({ type: "abort" });
    release();
    await pending;
    expect(f.client.getSnapshot().sessions[0].busy).toBe(false);
    expect(f.client.getSnapshot().pending).toBe(false);
  } finally {
    f.stop();
  }
});

test("transport lease replacement retains drafts only within same connection", async () => {
  const f = fixture();
  try {
    await signal(f.client, () => f.client.getSnapshot().messages.length === 1);
    f.client.setDraft("retained");
    f.stop();
    const stop = f.client.start({ ...f.transport });
    expect(f.client.getSnapshot().drafts.s1).toBe("retained");
    stop();
    const stopOther = f.client.start({ ...f.transport, scopeKey: "other" });
    expect(f.client.getSnapshot().drafts).toEqual({});
    stopOther();
  } finally {
    f.stop();
  }
});
