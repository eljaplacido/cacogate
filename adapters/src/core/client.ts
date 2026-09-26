// Daemon client plus the fail-safe fallback used when the daemon is unreachable.
import type { ActionEnvelope } from "./envelope.js";

export type Verdict =
  | "allow"
  | "sandbox"
  | "canary"
  | "revise"
  | "request_evidence"
  | "ask_human"
  | "block";

export type Mode = "shadow" | "enforce";

/** What the harness should do, independent of its wire format. */
export type Enforcement = "defer" | "ask" | "deny";

export interface Outcome {
  verdict: Verdict;
  mode: Mode;
  packet: string;
  enforcement: Enforcement;
  decisionId?: string;
  degraded?: string;
}

export interface ClientOptions {
  addr?: string;
  timeoutMs?: number;
  mode?: Mode;
  fetchImpl?: typeof fetch;
}

const READ_TOOLS = new Set([
  "read",
  "glob",
  "grep",
  "list",
  "ls",
  "find",
  "view",
  "search",
  "codesearch",
  "lsp",
]);

/** Mirrors crates/cacogate-harness enforcement(): shadow never interferes. */
export function enforcement(verdict: Verdict, mode: Mode): Enforcement {
  if (mode === "shadow" || verdict === "allow") return "defer";
  if (verdict === "sandbox" || verdict === "canary" || verdict === "ask_human") return "ask";
  return "deny";
}

export function resolveOptions(o: ClientOptions = {}): Required<Omit<ClientOptions, "fetchImpl">> {
  const env = typeof process === "undefined" ? {} : process.env;
  const mode = (o.mode ?? env.CACOGATE_MODE ?? "shadow") === "enforce" ? "enforce" : "shadow";
  return {
    addr: o.addr ?? env.CACOGATE_ADDR ?? "127.0.0.1:4320",
    timeoutMs: o.timeoutMs ?? 3000,
    mode,
  };
}

/**
 * Ask the daemon. Without the daemon the adapter cannot parse commands, so it
 * falls back to the strictest safe answer: read-only tools pass, everything
 * else is refused in enforce mode. Shadow mode never interferes.
 */
export async function checkAction(env: ActionEnvelope, o: ClientOptions = {}): Promise<Outcome> {
  const opts = resolveOptions(o);
  const f = o.fetchImpl ?? fetch;
  try {
    const res = await f(`http://${opts.addr}/v1/check`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(env),
      signal: AbortSignal.timeout(opts.timeoutMs),
    });
    if (!res.ok) throw new Error(`daemon answered ${res.status}: ${await res.text()}`);
    const body = (await res.json()) as {
      decision: { verdict: Verdict; mode?: Mode; decision_id: string };
      packet: string;
    };
    const mode = body.decision.mode ?? opts.mode;
    return {
      verdict: body.decision.verdict,
      mode,
      packet: body.packet,
      enforcement: enforcement(body.decision.verdict, mode),
      decisionId: body.decision.decision_id,
    };
  } catch (err) {
    const reason = err instanceof Error ? err.message : String(err);
    const read = READ_TOOLS.has(env.tool.raw_name.toLowerCase());
    const verdict: Verdict = read ? "allow" : "block";
    const packet = [
      `cacogate ▸ ${read ? "ALLOW" : "BLOCK"}  (daemon unreachable)   ${env.tool.raw_name}`,
      `  why     PRV-GATE-001: cacogate daemon unreachable (${reason}); fail-safe default`,
      "  do      start it with `cacogate daemon`, then retry",
    ].join("\n");
    return {
      verdict,
      mode: opts.mode,
      packet,
      enforcement: enforcement(verdict, opts.mode),
      degraded: reason,
    };
  }
}

export interface Execution {
  session_id: string;
  tool_use_id: string;
  executed: boolean;
  is_error?: boolean;
  exit_code?: number;
}

/** Report an execution; failures are swallowed (the gate never breaks the tool). */
export async function reportExecution(x: Execution, o: ClientOptions = {}): Promise<void> {
  const opts = resolveOptions(o);
  const f = o.fetchImpl ?? fetch;
  const body = {
    contract_version: "0.1.0",
    session_id: x.session_id,
    tool_use_id: x.tool_use_id,
    executed: x.executed,
    bucket: "result",
    ts: new Date().toISOString(),
    ...(x.is_error === undefined ? {} : { is_error: x.is_error }),
    ...(x.exit_code === undefined ? {} : { exit_code: x.exit_code }),
  };
  try {
    await f(`http://${opts.addr}/v1/execution`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(opts.timeoutMs),
    });
  } catch {
    // recorded as a gap by the absence of an execution entry
  }
}
