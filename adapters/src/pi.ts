// cacogate extension for pi and oh-my-pi (omp).
//
// Install: copy dist/pi/cacogate.js to .pi/extensions/ (project, must be
// trusted) or ~/.pi/agent/extensions/. Events used:
//   pi.on("tool_call", (event{toolCallId,toolName,input}, ctx) => {block, reason} | undefined)
//   pi.on("tool_result", (event{toolCallId,isError,…}, ctx) => undefined)
// `ask_human` uses ctx.ui.confirm when a UI exists, and blocks otherwise.
import { checkAction, reportExecution } from "./core/client.js";
import { buildEnvelope } from "./core/envelope.js";

interface ToolCallEvent {
  toolCallId: string;
  toolName: string;
  input: Record<string, unknown>;
}

interface ToolResultEvent {
  toolCallId: string;
  toolName: string;
  isError: boolean;
  details?: unknown;
}

interface Ctx {
  cwd?: string;
  hasUI?: boolean;
  ui?: { confirm?: (title: string, message: string) => Promise<boolean> };
  sessionManager?: { getSessionId?: () => string; getSessionFile?: () => string | undefined };
}

interface ExtensionAPI {
  on(event: "tool_call", h: (e: ToolCallEvent, ctx: Ctx) => Promise<unknown>): void;
  on(event: "tool_result", h: (e: ToolResultEvent, ctx: Ctx) => Promise<unknown>): void;
}

const fallbackSession = `pi-${process.pid}-${Date.now()}`;

function sessionOf(ctx: Ctx): string {
  return (
    ctx.sessionManager?.getSessionId?.() ??
    ctx.sessionManager?.getSessionFile?.() ??
    fallbackSession
  );
}

// omp runs under bun with argv[1] = .../oh-my-pi/.../cli.ts and sets
// process.title = "omp"; upstream pi sets process.title = "pi".
export function harnessName(
  title: string = process.title,
  argv: readonly string[] = process.argv,
): "pi" | "omp" {
  if (title === "omp") return "omp";
  return argv.some((a) => /(^|[/\\])(omp|oh-my-pi)([/\\.]|$)/.test(a)) ? "omp" : "pi";
}

export default function cacogate(pi: ExtensionAPI): void {
  pi.on("tool_call", async (event, ctx) => {
    const env = buildEnvelope({
      harness: harnessName(),
      interactive: ctx.hasUI === true,
      sessionId: sessionOf(ctx),
      toolUseId: event.toolCallId,
      tool: event.toolName,
      args: event.input ?? {},
      cwd: ctx.cwd ?? process.cwd(),
    });
    const o = await checkAction(env);
    if (o.enforcement === "defer") return undefined;
    if (o.enforcement === "ask" && ctx.hasUI && ctx.ui?.confirm) {
      const ok = await ctx.ui.confirm("cacogate: approve this action?", o.packet);
      return ok
        ? undefined
        : { block: true, reason: `${o.packet}\n  note    rejected by the user` };
    }
    return { block: true, reason: o.packet };
  });

  pi.on("tool_result", async (event, ctx) => {
    await reportExecution({
      session_id: sessionOf(ctx),
      tool_use_id: event.toolCallId,
      executed: true,
      is_error: event.isError,
    });
    return undefined;
  });
}
