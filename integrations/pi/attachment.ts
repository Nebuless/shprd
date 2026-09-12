import { createServer, type Socket } from "node:net";
import { chmod, lstat, mkdir, rm, writeFile } from "node:fs/promises";
import { randomBytes, createHash, timingSafeEqual } from "node:crypto";
import { homedir, userInfo } from "node:os";
import { join } from "node:path";
import { once } from "node:events";
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const LIMIT = 1024 * 1024;
const exec = promisify(execFile);
export interface NativeModel {
  readonly provider: string;
  readonly id: string;
  readonly name: string;
}
export interface NativeContext<M extends NativeModel> {
  readonly cwd: string;
  readonly hasUI: boolean;
  readonly model: M | undefined;
  isIdle(): boolean;
  abort(): void;
  readonly sessionManager: {
    getSessionId(): string;
    getSessionName(): string | undefined;
    getBranch(): ReadonlyArray<{
      readonly type: string;
      readonly message?: unknown;
    }>;
  };
  readonly modelRegistry: {
    getAvailable(): M[];
    find(provider: string, modelId: string): M | undefined;
  };
  readonly ui: {
    confirm(
      title: string,
      message: string,
      options?: { signal?: AbortSignal; timeout?: number },
    ): Promise<boolean>;
    input(
      title: string,
      placeholder?: string,
      options?: { signal?: AbortSignal; timeout?: number },
    ): Promise<string | undefined>;
    select(
      title: string,
      options: string[],
      opts?: { signal?: AbortSignal; timeout?: number },
    ): Promise<string | undefined>;
  };
}
export interface NativeRuntime<M extends NativeModel> {
  prompt(message: string): void;
  setModel(model: M): Promise<boolean>;
  thinkingLevel(): string;
}
class AttachmentError extends Error {
  constructor(readonly code: string) {
    super(code);
  }
}
function object(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw new AttachmentError("INVALID_REQUEST");
  return Object.fromEntries(Object.entries(value));
}
function text(value: unknown, max = 65536): string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    Buffer.byteLength(value) > max
  )
    throw new AttachmentError("INVALID_REQUEST");
  return value;
}
export const defaultDirectory = () =>
  process.env.SHPRD_AGENT_DIR ?? join(homedir(), ".shprd", "attachments");

async function privateDirectory(path: string) {
  await mkdir(path, { recursive: true, mode: 0o700 });
  const stat = await lstat(path);
  if (!stat.isDirectory() || stat.isSymbolicLink())
    throw new AttachmentError("UNSAFE_DIRECTORY");
  if (process.platform === "win32") {
    await exec("icacls.exe", [
      path,
      "/inheritance:r",
      "/grant:r",
      userInfo().username + ":(OI)(CI)F",
    ]);
  } else {
    if (stat.uid !== process.getuid?.())
      throw new AttachmentError("UNSAFE_DIRECTORY");
    await chmod(path, 0o700);
  }
}

/** Runs inside the existing engine. Never opens session files or starts an engine. */
export function createAttachment<M extends NativeModel>(
  agent: "senpi" | "atomic",
  runtime: NativeRuntime<M>,
  directory = defaultDirectory(),
) {
  let ctx: NativeContext<M> | undefined;
  let ownerPath: string | undefined;
  let server: ReturnType<typeof createServer> | undefined;
  let sessionId = "";
  let token = "";
  const peers = new Map<
    Socket,
    { authenticated: boolean; active: number; controller: AbortController }
  >();
  const send = (socket: Socket, frame: unknown) => {
    const wire = JSON.stringify(frame) + "\n";
    if (Buffer.byteLength(wire) > LIMIT || socket.writableLength > LIMIT) {
      socket.destroy();
      return;
    }
    socket.write(wire);
  };
  const emit = (event: unknown) => {
    for (const [socket, peer] of peers)
      if (peer.authenticated)
        send(socket, { agent_event: { session_id: sessionId, event } });
  };
  const state = (current: NativeContext<M>) => ({
    id: sessionId,
    agent,
    name: current.sessionManager.getSessionName() ?? "",
    cwd: current.cwd,
    connected: true,
    busy: !current.isIdle(),
    model: current.model
      ? {
          provider: current.model.provider,
          modelId: current.model.id,
          name: current.model.name,
        }
      : null,
    thinkingLevel: runtime.thinkingLevel(),
    capabilities: {
      ui_dialog: current.hasUI,
      existing_dialog_response: false,
      custom_ui: "terminal_handoff",
    },
  });
  const dispatch = async (
    command: Record<string, unknown>,
    current: NativeContext<M>,
    signal: AbortSignal,
  ) => {
    switch (command.type) {
      case "get_state":
        return state(current);
      case "get_messages":
        return {
          messages: current.sessionManager
            .getBranch()
            .filter((entry) => entry.type === "message")
            .map((entry) => entry.message),
        };
      case "get_available_models":
        return {
          models: current.modelRegistry.getAvailable().map((model) => ({
            provider: model.provider,
            modelId: model.id,
            name: model.name,
          })),
        };
      case "prompt":
        runtime.prompt(text(command.message));
        return { accepted: true };
      case "abort":
        current.abort();
        emit({ type: "abort_requested" });
        return { accepted: true };
      case "set_model": {
        const model = current.modelRegistry.find(
          text(command.provider, 256),
          text(command.modelId, 256),
        );
        if (!model) throw new AttachmentError("MODEL_NOT_FOUND");
        if (!(await runtime.setModel(model)))
          throw new AttachmentError("MODEL_UNAVAILABLE");
        return { accepted: true };
      }
      case "ui_dialog": {
        if (!current.hasUI) throw new AttachmentError("TERMINAL_HANDOFF");
        const title = text(command.title, 4096);
        const dialogId = randomBytes(16).toString("hex");
        const options = { signal, timeout: 30000 };
        let dialog: () => Promise<string | boolean | undefined>;
        switch (command.kind) {
          case "confirm": {
            const message = text(command.message, 4096);
            dialog = () => current.ui.confirm(title, message, options);
            break;
          }
          case "input": {
            const placeholder =
              command.placeholder === undefined
                ? ""
                : text(command.placeholder, 4096);
            dialog = () => current.ui.input(title, placeholder, options);
            break;
          }
          case "select": {
            if (
              !Array.isArray(command.options) ||
              command.options.length < 1 ||
              command.options.length > 100
            )
              throw new AttachmentError("INVALID_REQUEST");
            const choices = command.options.map((option) => text(option, 4096));
            dialog = () => current.ui.select(title, choices, options);
            break;
          }
          default:
            throw new AttachmentError("TERMINAL_HANDOFF");
        }
        emit({
          type: "ui_dialog_request",
          dialog_id: dialogId,
          kind: command.kind,
          title,
          response_owner: "native_ui",
        });
        const value = (await dialog()) ?? null;
        if (signal.aborted) throw new AttachmentError("SESSION_CHANGED");
        emit({ type: "ui_dialog_response", dialog_id: dialogId, value });
        return { value };
      }
      default:
        throw new AttachmentError("UNSUPPORTED_COMMAND");
    }
  };
  const stop = async () => {
    const oldServer = server;
    server = undefined;
    ctx = undefined;
    for (const [socket, peer] of peers) {
      peer.controller.abort();
      socket.destroy();
    }
    peers.clear();
    if (oldServer?.listening)
      await new Promise<void>((resolve, reject) =>
        oldServer.close((error) => (error ? reject(error) : resolve())),
      );
    if (ownerPath) {
      const oldPath = ownerPath;
      ownerPath = undefined;
      await rm(oldPath, { recursive: true, force: true });
    }
  };
  const start = async (current: NativeContext<M>) => {
    await stop();
    sessionId = agent + ":" + current.sessionManager.getSessionId();
    const key = createHash("sha256")
      .update(sessionId)
      .digest("hex")
      .slice(0, 32);
    await privateDirectory(directory);
    const path = join(directory, key);
    // Exclusive directory is the live attachment lease. Never steal a stale lease.
    await mkdir(path, { mode: 0o700 });
    ownerPath = path;
    try {
      await privateDirectory(path);
      token = randomBytes(32).toString("hex");
      const endpoint =
        process.platform === "win32"
          ? "\\\\.\\pipe\\shprd-" + key + "-" + randomBytes(8).toString("hex")
          : join(path, "control.sock");
      if (process.platform !== "win32" && Buffer.byteLength(endpoint) > 103)
        throw new AttachmentError("SOCKET_PATH_TOO_LONG");
      ctx = current;
      server = createServer((socket) => {
        if (peers.size >= 16) {
          socket.destroy();
          return;
        }
        const peer = {
          authenticated: false,
          active: 0,
          controller: new AbortController(),
        };
        peers.set(socket, peer);
        socket.setTimeout(5000, () => socket.destroy());
        socket.on("error", () => socket.destroy());
        socket.on("close", () => {
          peer.controller.abort();
          peers.delete(socket);
        });
        let buffer = Buffer.alloc(0);
        const handle = async (line: Buffer) => {
          let id: string | null = null;
          try {
            const frame = object(JSON.parse(line.toString("utf8")));
            id = text(frame.id, 128);
            const supplied = Buffer.from(text(frame.token, 128));
            const expected = Buffer.from(token);
            if (
              supplied.length !== expected.length ||
              !timingSafeEqual(supplied, expected) ||
              frame.session_id !== sessionId
            )
              throw new AttachmentError("UNAUTHORIZED");
            peer.authenticated = true;
            socket.setTimeout(0);
            const active = ctx;
            if (
              !active ||
              active.sessionManager.getSessionId() !==
                sessionId.slice(agent.length + 1)
            )
              throw new AttachmentError("SESSION_CHANGED");
            const result = await dispatch(
              object(frame.command),
              active,
              peer.controller.signal,
            );
            const wire = JSON.stringify({ id, result });
            if (Buffer.byteLength(wire) + 1 > LIMIT)
              throw new AttachmentError("RESPONSE_TOO_LARGE");
            send(socket, { id, result });
          } catch (error) {
            // Boundary errors never expose native exception text or model credentials.
            send(socket, {
              id,
              error: {
                code:
                  error instanceof AttachmentError
                    ? error.code
                    : "NATIVE_ERROR",
              },
            });
            if (!peer.authenticated) socket.end();
          } finally {
            peer.active--;
          }
        };
        socket.on("data", (chunk) => {
          buffer = Buffer.concat([
            buffer,
            typeof chunk === "string" ? Buffer.from(chunk) : chunk,
          ]);
          if (buffer.length > LIMIT) {
            socket.destroy();
            return;
          }
          for (
            let end = buffer.indexOf(10);
            end >= 0;
            end = buffer.indexOf(10)
          ) {
            const line = buffer.subarray(0, end);
            buffer = buffer.subarray(end + 1);
            if (peer.active >= 16) {
              socket.destroy();
              return;
            }
            peer.active++;
            void handle(line);
          }
        });
      });
      server.listen(endpoint);
      await once(server, "listening");
      server.on("error", () => {
        for (const socket of peers.keys()) socket.destroy();
      });
      if (process.platform !== "win32") await chmod(endpoint, 0o600);
      await writeFile(
        join(path, "attachment.json"),
        JSON.stringify({
          version: 1,
          session_id: sessionId,
          agent,
          endpoint,
          token,
        }),
        { mode: 0o600, flag: "wx" },
      );
    } catch (error) {
      await stop();
      throw error;
    }
  };
  return {
    start,
    stop,
    event(event: unknown, current: NativeContext<M>) {
      ctx = current;
      emit(event);
    },
  };
}
