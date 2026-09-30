// cacogate plugin for OpenCode.
//
// Install: copy dist/opencode/cacogate.js to .opencode/plugin/ (project) or
// ~/.config/opencode/plugin/ (global). Hooks used (from @opencode-ai/plugin):
//   "tool.execute.before"(input{tool,sessionID,callID}, output{args}) — throw to block
//   "tool.execute.after"(input{tool,sessionID,callID,args}, output{title,output,metadata})
// OpenCode has no ask-from-hook API, so `ask_human` is refused with the packet
// until cacogate's approval queue lands (P1). That limit is declared, not hidden.

import { execFileSync } from "node:child_process";
import { checkAction, reportExecution } from "./core/client.js";
import { buildEnvelope, type Model } from "./core/envelope.js";

interface PluginInput {
  directory: string;
  worktree: string;
}

interface BeforeInput {
  tool: string;
  sessionID: string;
  callID: string;
}

interface AfterOutput {
  title?: string;
  output?: string;
  metadata?: Record<string, unknown>;
}

interface ChatParamsInput {
  sessionID: string;
  model?: { id?: string; providerID?: string };
}

/** OpenCode's own version: the running binary answers `--version` (once, at load). */
function opencodeVersion(): string | undefined {
  try {
    const out = execFileSync(process.execPath, ["--version"], {
      timeout: 2000,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    return /^\d+\.\d+/.test(out) ? out.split(/\s+/)[0] : undefined;
  } catch {
    return undefined;
  }
}

export const CacogatePlugin = async ({ directory }: PluginInput) => {
  const harnessVersion = opencodeVersion();
  // The model is not on tool events; chat.params names it for each turn.
  const models = new Map<string, Model>();
  return {
    "chat.params": async (input: ChatParamsInput) => {
      if (input?.sessionID && input.model?.id) {
        models.set(input.sessionID, {
          id: input.model.id,
          ...(input.model.providerID ? { endpoint_kind: input.model.providerID } : {}),
        });
      }
    },
    "tool.execute.before": async (
      input: BeforeInput,
      output: { args: Record<string, unknown> },
    ) => {
      const env = buildEnvelope({
        harness: "opencode",
        ...(harnessVersion ? { harnessVersion } : {}),
        model: models.get(input.sessionID),
        sessionId: input.sessionID,
        toolUseId: input.callID,
        tool: input.tool,
        args: output.args ?? {},
        cwd: directory,
      });
      const o = await checkAction(env);
      if (o.enforcement === "deny") throw new Error(o.packet);
      if (o.enforcement === "ask") {
        throw new Error(
          `${o.packet}\n  note    needs a person's approval; OpenCode hooks cannot ask`,
        );
      }
    },
    "tool.execute.after": async (input: BeforeInput & { args?: unknown }, output: AfterOutput) => {
      const meta = output?.metadata ?? {};
      const exit = typeof meta.exit === "number" ? meta.exit : undefined;
      await reportExecution({
        session_id: input.sessionID,
        tool_use_id: input.callID,
        executed: true,
        ...(exit === undefined ? {} : { exit_code: exit, is_error: exit !== 0 }),
      });
    },
  };
};

export default CacogatePlugin;
