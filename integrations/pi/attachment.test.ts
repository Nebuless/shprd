import { test, expect } from "bun:test";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createConnection } from "node:net";
import { once } from "node:events";
import { createAttachment } from "./attachment";

const integrationDirectory = fileURLToPath(new URL(".", import.meta.url));

test("variant manifests register exactly one matching adapter", async () => {
  for (const [manifest, entrypoint] of [
    ["package.json", "./senpi.ts"],
    ["atomic.package.json", "./atomic.ts"],
  ]) {
    const parsed = JSON.parse(
      await readFile(join(integrationDirectory, manifest), "utf8"),
    ) as { pi?: { extensions?: unknown } };
    expect(parsed.pi?.extensions).toEqual([entrypoint]);
  }
});

for (const agent of ["senpi", "atomic"] as const) {
  test(agent +
    " attaches current owner, authenticates and reconnects", async () => {
    const directory = await mkdtemp(join(tmpdir(), "shprd-attach-"));
    const model = { provider: "fixture", id: "free", name: "Fixture" };
    const messages = [{ role: "user", content: "existing" }];
    let busy = false;
    const admitted: Array<{
      message: string;
      image?: { mimeType: string; data: string };
    }> = [];
    const selected: string[] = [];
    const runtime = {
      imagePrompt: agent === "atomic",
      prompt(message: string, image?: { mimeType: string; data: string }) {
        admitted.push({ message, image });
        busy = true;
      },
      async setModel(value: typeof model) {
        selected.push(value.id);
        return true;
      },
      thinkingLevel() {
        return "off";
      },
    };
    const ctx = {
      cwd: directory,
      hasUI: true,
      model,
      isIdle: () => !busy,
      abort() {
        busy = false;
      },
      sessionManager: {
        getSessionId: () => "fixture-session",
        getSessionName: () => "Fixture",
        getBranch: () =>
          messages.map((message) => ({ type: "message", message })),
      },
      modelRegistry: {
        getAvailable: () => [model],
        find: (provider: string, id: string) =>
          provider === model.provider && id === model.id ? model : undefined,
      },
      ui: {
        async confirm() {
          return true;
        },
        async input() {
          return "answer";
        },
        async select() {
          return "one";
        },
      },
    };
    const owner = createAttachment(agent, runtime, directory);
    const duplicate = createAttachment(agent, runtime, directory);
    const sockets: ReturnType<typeof createConnection>[] = [];
    try {
      await owner.start(ctx);
      await expect(duplicate.start(ctx)).rejects.toThrow();
      const [entry] = await readdir(directory);
      if (!entry) throw new Error("missing discovery");
      const discovery = JSON.parse(
        await readFile(join(directory, entry, "attachment.json"), "utf8"),
      );
      const connect = async () => {
        const socket = createConnection(discovery.endpoint);
        sockets.push(socket);
        await once(socket, "connect");
        let buffer = "";
        const frames: unknown[] = [];
        const waiters: Array<(value: unknown) => void> = [];
        socket.on("data", (chunk) => {
          buffer += chunk.toString();
          for (
            let end = buffer.indexOf("\n");
            end >= 0;
            end = buffer.indexOf("\n")
          ) {
            const value: unknown = JSON.parse(buffer.slice(0, end));
            buffer = buffer.slice(end + 1);
            const resolve = waiters.shift();
            if (resolve) resolve(value);
            else frames.push(value);
          }
        });
        const next = () =>
          frames.length
            ? Promise.resolve(frames.shift())
            : new Promise<unknown>((resolve) => waiters.push(resolve));
        const request = (command: object, token = discovery.token) => {
          const response = next();
          socket.write(
            JSON.stringify({
              id: "request",
              token,
              session_id: discovery.session_id,
              command,
            }) + "\n",
          );
          return response;
        };
        return { socket, next, request };
      };
      const client = await connect();
      expect(await client.request({ type: "get_state" })).toMatchObject({
        result: {
          id: agent + ":fixture-session",
          connected: true,
          busy: false,
          capabilities: { image_prompt: agent === "atomic" },
        },
      });
      expect(
        await client.request({ type: "prompt", message: "" }),
      ).toMatchObject({ error: { code: "INVALID_REQUEST" } });
      expect(
        await client.request({
          type: "set_model",
          provider: "fixture",
          modelId: "missing",
        }),
      ).toMatchObject({ error: { code: "MODEL_NOT_FOUND" } });
      expect(
        await client.request({
          type: "set_model",
          provider: "fixture",
          modelId: "free",
        }),
      ).toMatchObject({ result: { accepted: true } });
      expect(selected).toEqual(["free"]);
      expect(
        await client.request({ type: "get_available_models" }),
      ).toMatchObject({
        result: { models: [{ provider: "fixture", modelId: "free" }] },
      });
      expect(
        await client.request({ type: "prompt", message: "hello" }),
      ).toMatchObject({ result: { accepted: true } });
      expect(admitted).toEqual([{ message: "hello" }]);
      if (agent === "atomic") {
        expect(
          await client.request({
            type: "prompt",
            message: "inspect image",
            image: {
              mimeType: "image/vnd.microsoft.icon",
              data: "iVBORw0KGgo=",
            },
          }),
        ).toMatchObject({ result: { accepted: true } });
        expect(admitted).toContainEqual({
          message: "inspect image",
          image: { mimeType: "image/x-icon", data: "iVBORw0KGgo=" },
        });
      } else {
        expect(
          await client.request({
            type: "prompt",
            message: "inspect image",
            image: { mimeType: "image/png", data: "iVBORw0KGgo=" },
          }),
        ).toMatchObject({ error: { code: "IMAGE_UNSUPPORTED" } });
      }
      expect(await client.request({ type: "abort" })).toMatchObject({
        agent_event: { event: { type: "abort_requested" } },
      });
      expect(await client.next()).toMatchObject({ result: { accepted: true } });
      expect(busy).toBe(false);
      const update = client.next();
      owner.event(
        {
          type: "message_update",
          assistantMessageEvent: { type: "text_delta", delta: "hi" },
        },
        ctx,
      );
      expect(await update).toMatchObject({
        agent_event: {
          session_id: agent + ":fixture-session",
          event: { type: "message_update" },
        },
      });
      const bad = await connect();
      const admittedBeforeBadRequest = admitted.slice();
      expect(
        await bad.request({ type: "prompt", message: "forbidden" }, "wrong"),
      ).toMatchObject({ error: { code: "UNAUTHORIZED" } });
      expect(admitted).toEqual(admittedBeforeBadRequest);
      client.socket.destroy();
      const second = await connect();
      expect(await second.request({ type: "get_messages" })).toMatchObject({
        result: { messages },
      });
      expect(
        await second.request({
          type: "ui_dialog",
          kind: "input",
          title: "Question",
        }),
      ).toMatchObject({
        agent_event: { event: { type: "ui_dialog_request" } },
      });
      expect(await second.next()).toMatchObject({
        agent_event: { event: { type: "ui_dialog_response", value: "answer" } },
      });
      expect(await second.next()).toMatchObject({
        result: { value: "answer" },
      });
      expect(
        await second.request({
          type: "ui_dialog",
          kind: "custom",
          title: "Fixture",
        }),
      ).toMatchObject({ error: { code: "TERMINAL_HANDOFF" } });
      const oversized = await connect();
      const closed = new Promise<void>((resolve) =>
        oversized.socket.once("close", () => resolve()),
      );
      oversized.socket.on("error", () => oversized.socket.destroy());
      oversized.socket.write("x".repeat(1024 * 1024 + 1));
      await closed;
    } finally {
      for (const socket of sockets) socket.destroy();
      await duplicate.stop();
      await owner.stop();
      expect(await readdir(directory)).toEqual([]);
      await rm(directory, { recursive: true });
    }
  }, 10000);
}
