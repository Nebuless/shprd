# Native runtime attachments

## Purpose
Expose supported extension APIs inside existing Senpi and Atomic engines.

## Ownership
- This directory owns minimal native runtime glue and isolated probes.
- Rust catalog, transport and host routing live in crates/shprd-agent.

## Local Contracts
- Never open session JSONL for writing or create another engine.
- Load exactly one matching variant entrypoint into each existing runtime.
- Discovery uses private SHPRD_AGENT_DIR or ~/.shprd/attachments.
- Socket frames require session identity and per-session token; never log tokens.
- Unsupported existing/custom TUI dialog responses remain terminal handoff.

## Verification
- Build crates/shprd-agent example probe before runtime-probe.integration.ts.
- Supply SENPI_PACKAGE and ATOMIC_PACKAGE installed package roots for native-loader tests.
- bun test integrations/pi/attachment.test.ts checks isolated socket behavior.
- bun test ./integrations/pi/runtime-probe.integration.ts checks installed native loaders with explicit package roots.

## Child DOX Index
None.
