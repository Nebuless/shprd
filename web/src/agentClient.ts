import type { ConnectionClient, ConnectionStatus } from "./api";

export type AgentSession = {
  id: string;
  agent: "senpi" | "atomic";
  name: string;
  cwd: string;
  connected: boolean;
  busy: boolean;
  capabilities: { image_prompt: boolean };
};
export type AgentModel = { provider: string; id: string; name: string };
export type AgentMessage = { key: string; role: string; content: unknown };
export type AgentImage = { mimeType: string; data: string };
export type AgentCommand =
  | { type: "get_state" | "get_messages" | "abort" | "get_available_models" }
  | { type: "prompt"; message: string; image?: AgentImage }
  | { type: "set_model"; provider: string; modelId: string };

export function record(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value))
    : {};
}

export function agentResponse(value: unknown): unknown {
  const response = record(value);
  if (response.success === false) {
    throw new Error(
      typeof response.error === "string"
        ? response.error
        : "Agent command failed",
    );
  }
  return response.type === "response" ? response.data : value;
}

export function parseAgentSessions(value: unknown): AgentSession[] {
  const sessions = record(value).sessions;
  if (!Array.isArray(sessions)) throw new Error("Invalid agent session list");
  const ids = new Set<string>();
  return sessions.map((value) => {
    const s = record(value);
    if (
      typeof s.id !== "string" ||
      !s.id ||
      ids.has(s.id) ||
      (s.agent !== "senpi" && s.agent !== "atomic") ||
      typeof s.name !== "string" ||
      typeof s.cwd !== "string" ||
      typeof s.connected !== "boolean" ||
      typeof s.busy !== "boolean"
    ) {
      throw new Error("Invalid agent session");
    }
    ids.add(s.id);
    return {
      id: s.id,
      agent: s.agent,
      name: s.name,
      cwd: s.cwd,
      connected: s.connected,
      busy: s.busy,
      capabilities: {
        image_prompt: record(s.capabilities).image_prompt === true,
      },
    };
  });
}

export function parseAgentMessages(value: unknown): AgentMessage[] {
  const messages = record(agentResponse(value)).messages;
  if (!Array.isArray(messages)) throw new Error("Invalid agent messages");
  return messages.map((value, index) => {
    const m = record(value);
    if (typeof m.role !== "string" || !("content" in m))
      throw new Error("Invalid agent message");
    return {
      key: String(m.id ?? m.timestamp ?? index) + ":" + index,
      role: m.role,
      content: m.content,
    };
  });
}

export function parseAgentModels(value: unknown): AgentModel[] {
  const models = record(agentResponse(value)).models;
  if (!Array.isArray(models)) throw new Error("Invalid agent model list");
  return models.map((value) => {
    const m = record(value);
    const id = m.modelId ?? m.id;
    if (
      typeof m.provider !== "string" ||
      typeof id !== "string" ||
      !m.provider ||
      !id
    ) {
      throw new Error("Invalid agent model");
    }
    return {
      provider: m.provider,
      id,
      name: typeof m.name === "string" ? m.name : id,
    };
  });
}

/** Coordinator supplies scoped raw agent_event envelopes; this client opens no socket. */
export type AgentTransport = {
  scopeKey: string;
  call: (method: string, params?: Record<string, unknown>) => Promise<unknown>;
  canFetchImageUrl: boolean;
  fetchImage: (url: string) => Promise<Blob>;
  subscribe: (listener: (envelope: unknown) => void) => () => void;
  onStatus: (listener: (status: ConnectionStatus) => void) => () => void;
};

export function imageFetchPath(
  connectionId: string,
  generation: number,
  url: string,
): string {
  return `/api/connections/${encodeURIComponent(connectionId)}/image-fetch?connection_generation=${encodeURIComponent(generation)}&url=${encodeURIComponent(url)}`;
}

export function createAgentTransport(
  client: ConnectionClient,
  subscribe: AgentTransport["subscribe"],
  onStatus: AgentTransport["onStatus"],
  canFetchImageUrl = false,
): AgentTransport {
  return {
    scopeKey: `${client.connectionId}:${client.generation}`,
    canFetchImageUrl,
    call: (method, params) => client.call(method, params),
    fetchImage: async (url) => {
      if (!canFetchImageUrl) throw new Error("Image URL fetch is unavailable.");
      if (!client.isCurrent())
        throw new Error("Connection changed during image fetch");
      const response = await fetch(
        imageFetchPath(client.connectionId, client.generation, url),
        { credentials: "same-origin" },
      );
      if (!client.isCurrent())
        throw new Error("Connection changed during image fetch");
      if (!response.ok) throw new Error("Could not fetch image");
      return response.blob();
    },
    subscribe: (listener) =>
      subscribe((value) => {
        const envelope = record(value);
        if (
          client.isCurrent() &&
          envelope.connection_id === client.connectionId &&
          client.acceptsServerGeneration(envelope.connection_generation)
        )
          listener(value);
      }),
    onStatus,
  };
}

export type AgentWorkspaceState = {
  sessions: AgentSession[];
  selectedId: string | null;
  messages: AgentMessage[];
  models: AgentModel[];
  model: string;
  drafts: Record<string, string>;
  connected: boolean;
  loading: boolean;
  pending: boolean;
  error: string;
};

export class AgentWorkspaceClient {
  private state: AgentWorkspaceState = {
    sessions: [],
    selectedId: null,
    messages: [],
    models: [],
    model: "",
    drafts: {},
    connected: false,
    loading: false,
    pending: false,
    error: "",
  };
  private listeners = new Set<() => void>();
  private epoch = 0;
  private listEpoch = 0;
  private eventRevision = 0;
  private queuedEvents: unknown[] = [];
  private commandEpoch = 0;
  private active = false;
  private attachmentQueue: Promise<void> = Promise.resolve();
  private attached: { id: string; transport: AgentTransport } | null = null;
  constructor(private transport: AgentTransport) {}
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private update(change: Partial<AgentWorkspaceState>) {
    this.state = { ...this.state, ...change };
    this.listeners.forEach((listener) => listener());
  }
  private fail(error: unknown) {
    this.update({
      error: error instanceof Error ? error.message : "Agent request failed",
    });
  }
  start = (transport = this.transport) => {
    if (transport.scopeKey !== this.transport.scopeKey) {
      this.update({
        sessions: [],
        selectedId: null,
        messages: [],
        models: [],
        model: "",
        drafts: {},
        error: "",
      });
    }
    this.transport = transport;
    this.active = true;
    const offEvent = this.transport.subscribe(this.onEvent);
    const offStatus = this.transport.onStatus((status) => {
      ++this.epoch;
      ++this.listEpoch;
      this.update({
        connected: status === "connected",
        loading: false,
        pending: false,
      });
      if (status === "connected") void this.refresh();
    });
    return () => {
      this.active = false;
      ++this.epoch;
      ++this.listEpoch;
      offEvent();
      offStatus();
      void this.watch(null).catch(() => {});
    };
  };
  refresh = async () => {
    const request = ++this.listEpoch;
    this.update({ error: "" });
    try {
      const sessions = parseAgentSessions(
        await this.transport.call("agent_control.list"),
      );
      if (!this.active || request !== this.listEpoch || !this.state.connected)
        return;
      const selectedId = sessions.some((s) => s.id === this.state.selectedId)
        ? this.state.selectedId
        : (sessions[0]?.id ?? null);
      this.update({ sessions });
      await this.select(selectedId);
    } catch (error) {
      if (this.active && request === this.listEpoch) this.fail(error);
    }
  };
  private request(id: string, command: AgentCommand) {
    return this.transport
      .call("agent_control.request", { session_id: id, command })
      .then(agentResponse);
  }
  private watch(id: string | null): Promise<void> {
    const transport = this.transport;
    this.attachmentQueue = this.attachmentQueue
      .catch(() => {})
      .then(async () => {
        if (this.attached) {
          const previous = this.attached;
          this.attached = null;
          await previous.transport.call("agent_control.unsubscribe", {
            session_id: previous.id,
          });
        }
        if (id) {
          await transport.call("agent_control.subscribe", { session_id: id });
          this.attached = { id, transport };
        }
      });
    return this.attachmentQueue;
  }
  select = async (id: string | null) => {
    const epoch = ++this.epoch;
    const revision = this.eventRevision;
    const session = this.state.sessions.find((s) => s.id === id);
    this.queuedEvents = [];
    this.update({
      selectedId: session?.id ?? null,
      messages: [],
      models: [],
      model: "",
      error: "",
      pending: false,
      loading: Boolean(session?.connected && this.state.connected),
    });
    try {
      await this.watch(
        session?.connected && this.state.connected ? session.id : null,
      );
    } catch (error) {
      if (this.active && epoch === this.epoch) {
        this.update({ loading: false });
        this.fail(error);
      }
      return;
    }
    if (
      !this.active ||
      epoch !== this.epoch ||
      !session?.connected ||
      !this.state.connected
    )
      return;
    const results = await Promise.allSettled([
      this.request(session.id, { type: "get_messages" }),
      this.request(session.id, { type: "get_state" }),
      this.request(session.id, { type: "get_available_models" }),
    ]);
    if (!this.active || epoch !== this.epoch) return;
    this.update({ loading: false });
    results.forEach((result, index) => {
      if (result.status === "rejected") {
        this.fail(result.reason);
        return;
      }
      try {
        if (index === 0)
          this.update({ messages: parseAgentMessages(result.value) });
        if (index === 1 && revision === this.eventRevision) {
          const state = record(result.value);
          const model = record(state.model);
          const modelId = model.modelId ?? model.id;
          this.update({
            model:
              typeof model.provider === "string" && typeof modelId === "string"
                ? JSON.stringify([model.provider, modelId])
                : "",
          });
          const busy = state.busy ?? state.isStreaming;
          if (typeof busy === "boolean") this.setBusy(session.id, busy);
        }
        if (index === 2)
          this.update({ models: parseAgentModels(result.value) });
      } catch (error) {
        this.fail(error);
      }
    });
    const queued = this.queuedEvents;
    this.queuedEvents = [];
    queued.forEach(this.onEvent);
  };
  setDraft = (text: string) => {
    if (this.state.selectedId)
      this.update({
        drafts: { ...this.state.drafts, [this.state.selectedId]: text },
      });
  };
  private setBusy(id: string, busy: boolean) {
    this.update({
      sessions: this.state.sessions.map((s) =>
        s.id === id ? { ...s, busy } : s,
      ),
    });
  }
  command = async (command: AgentCommand): Promise<boolean> => {
    const { selectedId, connected, pending } = this.state;
    const session = this.state.sessions.find((s) => s.id === selectedId);
    if (
      !selectedId ||
      !session?.connected ||
      !connected ||
      (pending && command.type !== "abort")
    )
      return false;
    if (command.type === "prompt" && (!command.message.trim() || session.busy))
      return false;
    if (
      command.type === "prompt" &&
      command.image &&
      !session.capabilities.image_prompt
    ) {
      this.fail(new Error("Image prompts are unavailable for this agent."));
      return false;
    }
    const epoch = this.epoch;
    const revision = this.eventRevision;
    const commandEpoch = ++this.commandEpoch;
    this.update({ pending: true, error: "" });
    try {
      await this.request(selectedId, command);
      if (
        !this.active ||
        epoch !== this.epoch ||
        commandEpoch !== this.commandEpoch
      )
        return false;
      switch (command.type) {
        case "prompt":
          if (this.state.drafts[selectedId] === command.message)
            this.setDraft("");
          if (revision === this.eventRevision) this.setBusy(selectedId, true);
          break;
        case "abort":
          break;
        case "set_model":
          this.update({
            model: JSON.stringify([command.provider, command.modelId]),
          });
          break;
        default:
          break;
      }
      return true;
    } catch (error) {
      if (
        this.active &&
        epoch === this.epoch &&
        commandEpoch === this.commandEpoch
      )
        this.fail(error);
      return false;
    } finally {
      if (
        this.active &&
        epoch === this.epoch &&
        commandEpoch === this.commandEpoch
      )
        this.update({ pending: false });
    }
  };
  private onEvent = (value: unknown) => {
    if (!this.active || !this.state.connected) return;
    const envelope = record(record(value).agent_event);
    const id = envelope.session_id;
    if (typeof id !== "string") return;
    const event = record(envelope.event);
    if (event.type === "attachment_lost") {
      this.update({
        sessions: this.state.sessions.map((session) =>
          session.id === id ? { ...session, connected: false } : session,
        ),
      });
      if (id === this.state.selectedId) {
        ++this.epoch;
        this.update({
          loading: false,
          pending: false,
          error:
            "Agent connection lost. Refresh to reconnect; unsent draft is retained.",
        });
      }
      return;
    }
    if (id === this.state.selectedId && this.state.loading) {
      this.queuedEvents.push(value);
      return;
    }
    if (event.type === "agent_start") this.setBusy(id, true);
    if (event.type === "agent_end" || event.type === "agent_settled")
      this.setBusy(id, false);
    if (id !== this.state.selectedId) return;
    ++this.eventRevision;
    const message = record(event.message);
    if (typeof message.role === "string" && "content" in message) {
      const messages = [...this.state.messages];
      const replace =
        event.type === "message_update" || event.type === "message_end";
      const last = messages[messages.length - 1];
      const next = {
        key: replace && last ? last.key : String(messages.length),
        role: message.role,
        content: message.content,
      };
      if (replace && last?.role === message.role)
        messages[messages.length - 1] = next;
      else messages.push(next);
      this.update({ messages });
    }
    if (
      event.type === "extension_ui_request" ||
      event.type === "ui_dialog_request"
    )
      this.update({
        error:
          "This extension needs native terminal interaction. Open terminal to continue.",
      });
    if (event.type === "error")
      this.update({
        error:
          typeof event.message === "string"
            ? event.message
            : "Agent reported an error",
      });
  };
}
