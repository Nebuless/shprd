import { test, expect } from "bun:test";
import { mkdtemp, rm, readdir } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";

const variants = [
  ["senpi", process.env.SENPI_PACKAGE],
  ["atomic", process.env.ATOMIC_PACKAGE],
] as const;
for (const [agent, installed] of variants) {
  test(agent +
    " installed loader binds existing runtime to Rust socket client", async () => {
    if (!installed)
      throw new Error(
        "Set SENPI_PACKAGE and ATOMIC_PACKAGE to installed package roots",
      );
    const { loadExtensions } = await import(
      join(installed, "dist/core/extensions/loader.js")
    );
    const directory = await mkdtemp(join(tmpdir(), "shprd-runtime-"));
    const previous = process.env.SHPRD_AGENT_DIR;
    process.env.SHPRD_AGENT_DIR = directory;
    let result: Awaited<ReturnType<typeof loadExtensions>> | undefined;
    const calls: string[] = [];
    const ctx = {
      cwd: directory,
      hasUI: false,
      model: undefined,
      isIdle: () => true,
      abort: () => calls.push("abort"),
      sessionManager: {
        getSessionId: () => "runtime-fixture",
        getSessionName: () => "Loader fixture",
        getBranch: () => [
          {
            type: "message",
            message: { role: "user", content: "fixture history" },
          },
        ],
      },
      modelRegistry: {
        getAvailable: () => [{ provider: "fixture", id: "free", name: "Free" }],
        find: () => undefined,
      },
    };
    const emit = async (type: string) => {
      for (const extension of result?.extensions ?? []) {
        for (const handler of extension.handlers.get(type) ?? [])
          await handler(
            { type, reason: type === "session_start" ? "startup" : "quit" },
            ctx,
          );
      }
    };
    try {
      result = await loadExtensions(
        [join(import.meta.dir, agent + ".ts")],
        directory,
      );
      expect(result.errors).toEqual([]);
      expect(result.extensions).toHaveLength(1);
      result.runtime.getThinkingLevel = () => "off";
      result.runtime.sendUserMessage = (message: string, options: object) => {
        calls.push(message);
        expect(options).toEqual({
          deliverAs: "followUp",
          expandPromptTemplates: false,
        });
      };
      await emit("session_start");
      const probe = Bun.spawn(
        [join(import.meta.dir, "../../target/debug/examples/probe")],
        {
          env: { ...process.env, SHPRD_AGENT_DIR: directory },
          stdout: "pipe",
          stderr: "pipe",
        },
      );
      const [stdout, stderr, code] = await Promise.all([
        new Response(probe.stdout).text(),
        new Response(probe.stderr).text(),
        probe.exited,
      ]);
      expect(stderr).toBe("");
      expect(code).toBe(0);
      expect(JSON.parse(stdout)).toEqual({
        probe: "ok",
        session_id: agent + ":runtime-fixture",
      });
      expect(calls).toEqual(["fixture-only", "abort"]);
      await emit("session_shutdown");
      expect(await readdir(directory)).toEqual([]);
      await emit("session_start");
      expect(await readdir(directory)).toHaveLength(1);
    } finally {
      await emit("session_shutdown");
      if (previous === undefined) delete process.env.SHPRD_AGENT_DIR;
      else process.env.SHPRD_AGENT_DIR = previous;
      await rm(directory, { recursive: true, force: true });
    }
  }, 20000);
}
