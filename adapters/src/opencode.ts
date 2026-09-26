// cacogate plugin for OpenCode.
//
// Install: copy dist/opencode/cacogate.js to .opencode/plugin/ (project) or
// ~/.config/opencode/plugin/ (global). Hooks used (from @opencode-ai/plugin):
//   "tool.execute.before"(input{tool,sessionID,callID}, output{args}) — throw to block
//   "tool.execute.after"(input{tool,sessionID,callID,args}, output{title,output,metadata})
// OpenCode has no ask-from-hook API, so `ask_human` is refused with the packet
// until cacogate's approval queue lands (P1). That limit is declared, not hidden.
import { checkAction, reportExecution } from "./core/client.js";
import { buildEnvelope } from "./core/envelope.js";

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

export const CacogatePlugin = async ({ directory }: PluginInput) => ({
  "tool.execute.before": async (input: BeforeInput, output: { args: Record<string, unknown> }) => {
    const env = buildEnvelope({
      harness: "opencode",
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
});

export default CacogatePlugin;
