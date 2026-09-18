import {
  useEffect,
  useId,
  useState,
  useRef,
  useMemo,
  useSyncExternalStore,
} from "react";
import {
  ArrowUp,
  Clipboard,
  ImagePlus,
  Link,
  MessageSquare,
  RefreshCw,
  Square,
  Terminal,
  X,
} from "lucide-react";
import {
  AgentWorkspaceClient,
  createAgentTransport,
  record,
  type AgentSession,
  type AgentTransport,
} from "../agentClient";
import {
  createEphemeralImageSlot,
  getSafeImageSourceLabel,
  readImageAttachment,
  type ImageAttachment,
} from "../agentImageAttachment";
import { bridge, type ConnectionClient } from "../api";
import { MarkdownPreview } from "./markdown";
import "./AgentWorkspace.css";

export type AgentWorkspaceProps = {
  transport: AgentTransport;
  onOpenTerminal: (session: AgentSession | null) => void;
};

export function ConnectedAgentWorkspace({
  client,
  onOpenTerminal,
}: {
  client: ConnectionClient;
  onOpenTerminal: AgentWorkspaceProps["onOpenTerminal"];
}) {
  const canFetchImageUrl = bridge.hello?.capabilities.image_url_fetch === true;
  const transport = useMemo(
    () =>
      createAgentTransport(
        client,
        (listener) =>
          bridge.onEvent((event) => {
            if (event.event === "agent_control.event")
              listener({
                ...event.data,
                connection_id: event.connection_id,
                connection_generation: event.connection_generation,
              });
          }),
        (listener) => bridge.onStatus(listener),
        canFetchImageUrl,
      ),
    [canFetchImageUrl, client],
  );
  return (
    <AgentWorkspace transport={transport} onOpenTerminal={onOpenTerminal} />
  );
}

function MessageContent({ content }: { content: unknown }) {
  if (typeof content === "string")
    return <MarkdownPreview text={content} breaks />;
  if (!Array.isArray(content))
    return <pre>{JSON.stringify(content, null, 2)}</pre>;
  return (
    <>
      {content.map((value, index) => {
        const block = record(value);
        const key = typeof block.id === "string" ? block.id : String(index);
        switch (block.type) {
          case "text":
            return typeof block.text === "string" ? (
              <MarkdownPreview key={key} text={block.text} breaks />
            ) : null;
          case "thinking":
            return (
              <details key={key}>
                <summary>Reasoning</summary>
                <pre>
                  {typeof block.thinking === "string"
                    ? block.thinking
                    : JSON.stringify(block, null, 2)}
                </pre>
              </details>
            );
          case "toolCall":
            return (
              <details key={key}>
                <summary>
                  Tool:{" "}
                  {typeof block.name === "string" ? block.name : "Unknown"}
                </summary>
                <pre>{JSON.stringify(block.arguments, null, 2)}</pre>
              </details>
            );
          default:
            return (
              <details key={key}>
                <summary>
                  {block.type === "image"
                    ? "Image attachment (open native terminal)"
                    : "Native content"}
                </summary>
                <pre>
                  {block.type === "image"
                    ? "Image data omitted from text view."
                    : JSON.stringify(block, null, 2)}
                </pre>
              </details>
            );
        }
      })}
    </>
  );
}

/** Parent owns bridge lease, raw event subscription, and native terminal focus. */
export function AgentWorkspace({
  transport,
  onOpenTerminal,
}: AgentWorkspaceProps) {
  const [client] = useState(() => new AgentWorkspaceClient(transport));
  const state = useSyncExternalStore(
    client.subscribe,
    client.getSnapshot,
    client.getSnapshot,
  );
  const id = useId();
  const fileInput = useRef<HTMLInputElement>(null);
  const attachmentSlot = useRef(createEphemeralImageSlot());
  type AttachedImage = {
    image: ImageAttachment;
    preview: string;
    label: string;
    owner: { sessionId: string; scopeKey: string };
  };
  const [attachment, setAttachment] = useState<AttachedImage | null>(null);
  const attachmentRef = useRef<AttachedImage | null>(null);
  const menuTrigger = useRef<HTMLButtonElement>(null);
  const [attachmentOpen, setAttachmentOpen] = useState(false);
  const [urlEntry, setUrlEntry] = useState(false);
  const [imageUrl, setImageUrl] = useState("");
  const [failedImageSource, setFailedImageSource] = useState<
    "clipboard" | "url" | null
  >(null);
  const [imageStatus, setImageStatus] = useState("");
  const [imagePending, setImagePending] = useState(false);
  const imageAttempt = useRef(0);
  const scroll = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  const session = state.sessions.find((s) => s.id === state.selectedId) ?? null;
  const draft = state.selectedId ? (state.drafts[state.selectedId] ?? "") : "";
  const usable = state.connected && Boolean(session?.connected);
  const imageSupported = usable && session?.capabilities.image_prompt === true;
  const imageUrlSupported = imageSupported && transport.canFetchImageUrl;
  attachmentRef.current = attachment;
  const imageOwner = useRef<{
    id: string | null;
    scopeKey: string;
    supported: boolean;
  }>({
    id: state.selectedId,
    scopeKey: transport.scopeKey,
    supported: imageSupported,
  });
  imageOwner.current = {
    id: state.selectedId,
    scopeKey: transport.scopeKey,
    supported: imageSupported,
  };
  const ownsImageAttempt = (
    attempt: number,
    owner: string | null,
    scopeKey: string,
  ) =>
    imageAttempt.current === attempt &&
    owner !== null &&
    imageOwner.current.id === owner &&
    imageOwner.current.scopeKey === scopeKey &&
    imageOwner.current.supported;
  const clearAttachment = (preview?: string) => {
    if (preview && attachmentRef.current?.preview !== preview) return;
    imageAttempt.current += 1;
    attachmentSlot.current.remove();
    setAttachment(null);
    setImagePending(false);
    setImageStatus("");
    setImageUrl("");
    setFailedImageSource(null);
  };
  const uploadImageUrl = async () => {
    const url = imageUrl.trim();
    if (!url) return;
    const owner = session?.id ?? null;
    const scopeKey = transport.scopeKey;
    const attempt = ++imageAttempt.current;
    try {
      const parsed = new URL(url);
      if (parsed.protocol !== "https:")
        throw new Error("Use a public HTTPS image URL.");
      setImagePending(true);
      setImageStatus("Fetching image URL...");
      const blob = await transport.fetchImage(url);
      await selectAttachment(blob, url, owner, scopeKey, attempt, "url");
    } catch (error) {
      if (!ownsImageAttempt(attempt, owner, scopeKey)) return;
      setFailedImageSource("url");
      setImageStatus(
        error instanceof Error ? error.message : "Could not fetch image URL.",
      );
      menuTrigger.current?.focus();
    } finally {
      if (ownsImageAttempt(attempt, owner, scopeKey)) setImagePending(false);
    }
  };
  const openFilePicker = () => {
    imageAttempt.current += 1;
    setImagePending(false);
    setAttachmentOpen(false);
    const restoreFocus = () => menuTrigger.current?.focus();
    window.addEventListener("focus", restoreFocus, { once: true });
    fileInput.current?.click();
  };
  const cancelImageUrl = () => {
    imageAttempt.current += 1;
    setImagePending(false);
    setUrlEntry(false);
    setFailedImageSource(null);
    setImageStatus("");
    menuTrigger.current?.focus();
  };
  const readClipboardImage = async () => {
    const owner = session?.id ?? null;
    const scopeKey = transport.scopeKey;
    const attempt = ++imageAttempt.current;
    setImagePending(true);
    setImageStatus("Reading clipboard image...");
    try {
      const items = await navigator.clipboard.read();
      const item = items
        .flatMap((entry) => entry.types.map((type) => ({ entry, type })))
        .find(({ type }) => type.startsWith("image/"));
      if (!item) throw new Error("Clipboard has no readable image.");
      await selectAttachment(
        await item.entry.getType(item.type),
        "Clipboard image",
        owner,
        scopeKey,
        attempt,
        "clipboard",
      );
    } catch (error) {
      if (!ownsImageAttempt(attempt, owner, scopeKey)) return;
      setFailedImageSource("clipboard");
      setImageStatus(
        error instanceof DOMException && error.name === "NotAllowedError"
          ? "Clipboard permission was denied."
          : error instanceof Error &&
              error.message === "Clipboard has no readable image."
            ? error.message
            : "Clipboard image access is unavailable.",
      );
      menuTrigger.current?.focus();
    } finally {
      if (ownsImageAttempt(attempt, owner, scopeKey)) setImagePending(false);
    }
  };
  const selectAttachment = async (
    blob: Blob,
    label: string,
    owner = session?.id ?? null,
    scopeKey = transport.scopeKey,
    attempt?: number,
    source?: "clipboard" | "url",
  ) => {
    const imageAttemptId = attempt ?? ++imageAttempt.current;
    setImagePending(true);
    setImageStatus("Preparing image...");
    try {
      const image = await readImageAttachment(blob);
      if (!owner || !ownsImageAttempt(imageAttemptId, owner, scopeKey)) return;
      const preview = attachmentSlot.current.set(URL.createObjectURL(blob));
      setAttachment({
        image,
        preview,
        label: getSafeImageSourceLabel(label),
        owner: { sessionId: owner, scopeKey },
      });
      setImageStatus("Image ready.");
      setFailedImageSource(null);
      setAttachmentOpen(false);
      setUrlEntry(false);
      menuTrigger.current?.focus();
    } catch (error) {
      if (!ownsImageAttempt(imageAttemptId, owner, scopeKey)) return;
      if (source) setFailedImageSource(source);
      setImageStatus(
        error instanceof Error ? error.message : "Could not prepare image",
      );
      menuTrigger.current?.focus();
    } finally {
      if (ownsImageAttempt(imageAttemptId, owner, scopeKey))
        setImagePending(false);
    }
  };
  useEffect(() => client.start(transport), [client, transport]);
  useEffect(() => {
    clearAttachment();
    // Selection, disconnect, and capability loss invalidate image ownership.
  }, [
    state.selectedId,
    state.connected,
    session?.connected,
    imageSupported,
    transport.scopeKey,
  ]);
  useEffect(() => () => attachmentSlot.current.remove(), []);
  useEffect(() => {
    void state.messages;
    if (follow.current && scroll.current)
      scroll.current.scrollTop = scroll.current.scrollHeight;
  }, [state.messages]);
  useEffect(() => {
    void state.selectedId;
    follow.current = true;
  }, [state.selectedId]);
  return (
    <section className="agent-workspace" aria-label="Agent workspace">
      <aside className="agent-workspace-sessions" aria-label="Agent sessions">
        <header className="agent-workspace-sidebar-header">
          <div>
            <span className="agent-workspace-eyebrow">SHPRD</span>
            <h2>
              Sessions <small>{state.sessions.length}</small>
            </h2>
          </div>
          <button
            type="button"
            onClick={() => void client.refresh()}
            disabled={!state.connected}
            title="Refresh sessions"
            aria-label="Refresh sessions"
          >
            <RefreshCw size={16} />
          </button>
        </header>
        <div className="agent-workspace-session-list">
          {state.sessions.map((item) => (
            <button
              type="button"
              key={item.id}
              aria-pressed={session?.id === item.id}
              className="agent-workspace-session"
              onClick={() => void client.select(item.id)}
            >
              <span className="agent-workspace-session-top">
                <MessageSquare size={16} />
                <strong>{item.name || item.id}</strong>
                <span
                  className={
                    item.connected
                      ? "agent-workspace-dot is-online"
                      : "agent-workspace-dot"
                  }
                />
              </span>
              <span className="agent-workspace-path" title={item.cwd}>
                {item.cwd}
              </span>
              <span className="agent-workspace-session-meta">
                <span>{item.agent === "senpi" ? "OmO / Senpi" : "Atomic"}</span>
                <span>
                  {!item.connected
                    ? "Offline"
                    : item.busy
                      ? "Working"
                      : "Ready"}
                </span>
              </span>
            </button>
          ))}
          {!state.sessions.length && (
            <p className="agent-workspace-note">
              {state.connected
                ? "No registered agent sessions. Start or attach an agent in the native terminal."
                : "Waiting for bridge connection."}
            </p>
          )}
        </div>
        <footer className="agent-workspace-sidebar-footer">
          <span
            className={
              state.connected
                ? "agent-workspace-dot is-online"
                : "agent-workspace-dot"
            }
          />
          {state.connected ? "Bridge connected" : "Bridge disconnected"}
        </footer>
      </aside>
      <div className="agent-workspace-conversation">
        <header className="agent-workspace-header">
          <div>
            <h2>{session?.name || "Agent workspace"}</h2>
            <p title={session?.cwd}>
              {session?.cwd || "Structured conversations, native tools"}
            </p>
          </div>
          <button
            type="button"
            aria-label="Open native terminal"
            onClick={() => onOpenTerminal(session)}
          >
            <Terminal size={16} />
            <span>Open terminal</span>
          </button>
        </header>
        {state.error && (
          <div className="agent-workspace-error" role="alert">
            {state.error}
            <button
              type="button"
              disabled={!state.connected}
              onClick={() => void client.refresh()}
            >
              Retry
            </button>
          </div>
        )}
        <div
          className="agent-workspace-messages"
          ref={scroll}
          tabIndex={0}
          role="region"
          aria-label="Conversation messages"
          onScroll={() => {
            const el = scroll.current;
            if (el)
              follow.current =
                el.scrollHeight - el.scrollTop - el.clientHeight < 48;
          }}
        >
          {state.loading ? (
            <p className="agent-workspace-note" role="status">
              Loading conversation...
            </p>
          ) : null}
          {!state.loading && !state.messages.length ? (
            <div className="agent-workspace-empty">
              <MessageSquare size={28} />
              <h3>
                {session
                  ? "Ready for your next task"
                  : "Your agents, one workspace"}
              </h3>
              <p>
                {session
                  ? "Send a message to this session. Responses and tool activity appear here."
                  : "Select an OmO / Senpi or Atomic session to view its conversation."}
              </p>
              <p>Custom TUI extensions stay in the native terminal.</p>
            </div>
          ) : null}
          {state.messages.map((message) => (
            <article
              className={
                message.role === "user"
                  ? "agent-workspace-message is-user"
                  : "agent-workspace-message"
              }
              key={message.key}
            >
              <header>
                {message.role === "assistant"
                  ? session?.agent === "atomic"
                    ? "Atomic"
                    : "OmO / Senpi"
                  : message.role === "toolResult"
                    ? "Tool result"
                    : message.role}
              </header>
              <MessageContent content={message.content} />
            </article>
          ))}
        </div>
        <form
          className="agent-workspace-composer"
          onSubmit={(event) => {
            event.preventDefault();
            if (imagePending) return;
            void (async () => {
              const sentAttachment = attachment;
              const currentAttachment =
                sentAttachment &&
                sentAttachment.owner.sessionId === session?.id &&
                sentAttachment.owner.scopeKey === transport.scopeKey
                  ? sentAttachment
                  : null;
              if (sentAttachment && !currentAttachment) {
                clearAttachment(sentAttachment.preview);
                setImageStatus("Image attachment expired. Add it again.");
                return;
              }
              const accepted = await client.command({
                type: "prompt",
                message: draft,
                ...(currentAttachment
                  ? { image: currentAttachment.image }
                  : {}),
              });
              if (accepted && currentAttachment)
                clearAttachment(currentAttachment.preview);
            })();
          }}
        >
          <div className="agent-workspace-status" role="status">
            {imageStatus ||
              (!usable
                ? "Session offline. Draft retained; reconnect to send."
                : state.pending
                  ? "Sending command..."
                  : session?.busy
                    ? "Agent working..."
                    : "Ready")}
          </div>
          {attachment ? (
            <div className="agent-workspace-image-preview">
              <img src={attachment.preview} alt={attachment.label} />
              <span>{attachment.label}</span>
              <button
                type="button"
                onClick={() => {
                  clearAttachment();
                  menuTrigger.current?.focus();
                }}
                aria-label="Remove image"
              >
                <X size={14} />
              </button>
            </div>
          ) : null}
          <input
            ref={fileInput}
            type="file"
            className="agent-workspace-sr-only"
            accept="image/png,image/jpeg,image/gif,image/webp,image/bmp,image/x-icon,image/vnd.microsoft.icon,image/avif"
            onChange={(event) => {
              const file = event.currentTarget.files?.[0];
              event.currentTarget.value = "";
              if (file) void selectAttachment(file, file.name);
            }}
          />
          <label className="agent-workspace-sr-only" htmlFor={id + "-message"}>
            Message agent
          </label>
          <textarea
            id={id + "-message"}
            value={draft}
            disabled={!session}
            onChange={(event) => client.setDraft(event.target.value)}
            rows={3}
            placeholder="Describe a task, ask a question..."
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                (event.ctrlKey || event.metaKey) &&
                !event.nativeEvent.isComposing
              ) {
                event.preventDefault();
                event.currentTarget.form?.requestSubmit();
              }
            }}
          />
          <div className="agent-workspace-composer-actions">
            <label className="agent-workspace-model" htmlFor={id + "-model"}>
              <span>Model</span>
              <select
                id={id + "-model"}
                value={state.model}
                disabled={
                  !usable ||
                  state.pending ||
                  session?.busy ||
                  !state.models.length
                }
                onChange={(event) => {
                  const model = state.models.find(
                    (m) =>
                      JSON.stringify([m.provider, m.id]) === event.target.value,
                  );
                  if (model)
                    void client.command({
                      type: "set_model",
                      provider: model.provider,
                      modelId: model.id,
                    });
                }}
              >
                <option value="">
                  {state.models.length ? "Select model" : "No models available"}
                </option>
                {state.model &&
                  !state.models.some(
                    (m) => JSON.stringify([m.provider, m.id]) === state.model,
                  ) && <option value={state.model}>Current model</option>}
                {state.models.map((m) => (
                  <option
                    key={JSON.stringify([m.provider, m.id])}
                    value={JSON.stringify([m.provider, m.id])}
                  >
                    {m.name} ({m.provider})
                  </option>
                ))}
              </select>
            </label>
            {imageSupported ? (
              <div className="agent-workspace-image-actions">
                <button
                  type="button"
                  ref={menuTrigger}
                  aria-expanded={attachmentOpen}
                  onClick={() => {
                    setAttachmentOpen((open) => !open);
                    setUrlEntry(false);
                  }}
                >
                  <ImagePlus size={16} /> Image
                </button>
                {attachmentOpen ? (
                  <div
                    className="agent-workspace-image-menu"
                    onKeyDown={(event) => {
                      if (event.key !== "Escape") return;
                      event.preventDefault();
                      setAttachmentOpen(false);
                      menuTrigger.current?.focus();
                    }}
                  >
                    <button type="button" onClick={openFilePicker}>
                      <ImagePlus size={14} /> Pick file
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setAttachmentOpen(false);
                        void readClipboardImage();
                      }}
                    >
                      <Clipboard size={14} /> Paste image
                    </button>
                    {imageUrlSupported ? (
                      <button
                        type="button"
                        onClick={() => {
                          setAttachmentOpen(false);
                          setUrlEntry(true);
                        }}
                      >
                        <Link size={14} /> Paste URL
                      </button>
                    ) : null}
                  </div>
                ) : null}
                {urlEntry && imageUrlSupported ? (
                  <label className="agent-workspace-image-url">
                    <span>HTTPS image URL</span>
                    <input
                      type="url"
                      autoFocus
                      value={imageUrl}
                      placeholder="https://example.com/image.png"
                      onChange={(event) =>
                        setImageUrl(event.currentTarget.value)
                      }
                      onKeyDown={(event) => {
                        if (event.key === "Escape") {
                          event.preventDefault();
                          cancelImageUrl();
                          return;
                        }
                        if (event.key !== "Enter") return;
                        event.preventDefault();
                        void uploadImageUrl();
                      }}
                    />
                    <button
                      type="button"
                      disabled={imagePending || !imageUrl.trim()}
                      onClick={() => void uploadImageUrl()}
                    >
                      Upload
                    </button>
                    <button type="button" onClick={cancelImageUrl}>
                      Cancel
                    </button>
                  </label>
                ) : null}
              </div>
            ) : null}
            {failedImageSource ? (
              <div className="agent-workspace-image-recovery">
                <button
                  type="button"
                  onClick={() => {
                    if (failedImageSource === "url") void uploadImageUrl();
                    else void readClipboardImage();
                  }}
                >
                  Retry {failedImageSource === "url" ? "URL" : "clipboard"}
                </button>
                <button type="button" onClick={openFilePicker}>
                  Replace image
                </button>
                <button
                  type="button"
                  onClick={() => {
                    clearAttachment();
                    menuTrigger.current?.focus();
                  }}
                >
                  Remove
                </button>
              </div>
            ) : null}
            <span className="agent-workspace-shortcut">Ctrl / Cmd + Enter</span>
            {session?.busy || state.pending ? (
              <button
                type="button"
                disabled={!usable}
                onClick={() => void client.command({ type: "abort" })}
              >
                <Square size={14} />
                Stop
              </button>
            ) : (
              <button
                type="submit"
                className="agent-workspace-send"
                disabled={
                  !usable || state.loading || imagePending || !draft.trim()
                }
              >
                <ArrowUp size={16} />
                Send
              </button>
            )}
          </div>
        </form>
      </div>
    </section>
  );
}
