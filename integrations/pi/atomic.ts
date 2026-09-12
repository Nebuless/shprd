import type { ExtensionAPI } from "@bastani/atomic";
import { createAttachment } from "./attachment";

export default function attachmentExtension(pi: ExtensionAPI) {
  const attachment = createAttachment("atomic", {
    prompt: (message) =>
      pi.sendUserMessage(message, {
        deliverAs: "followUp",
        expandPromptTemplates: false,
      }),
    setModel: (model: Parameters<ExtensionAPI["setModel"]>[0]) =>
      pi.setModel(model),
    thinkingLevel: () => pi.getThinkingLevel(),
  });
  pi.on("session_start", async (event, ctx) => {
    await attachment.start(ctx);
    attachment.event(event, ctx);
  });
  pi.on("session_shutdown", async (event, ctx) => {
    attachment.event(event, ctx);
    await attachment.stop();
  });
  pi.on("session_info_changed", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("session_compact", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("session_tree", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("agent_start", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("agent_end", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("agent_settled", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("message_start", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("message_update", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("message_end", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("tool_execution_start", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("tool_execution_update", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("tool_execution_end", (event, ctx) => {
    attachment.event(event, ctx);
  });
  pi.on("model_select", (event, ctx) => {
    attachment.event(
      {
        type: event.type,
        model: {
          provider: event.model.provider,
          modelId: event.model.id,
          name: event.model.name,
        },
      },
      ctx,
    );
  });
  pi.on("thinking_level_select", (event, ctx) => {
    attachment.event(event, ctx);
  });
}
