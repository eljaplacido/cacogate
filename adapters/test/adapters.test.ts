import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { checkAction, enforcement } from "../src/core/client.js";
import { actionDigest, buildEnvelope } from "../src/core/envelope.js";
import { digest } from "../src/core/jcs.js";
import { CacogatePlugin } from "../src/opencode.js";
import piExtension, { harnessName } from "../src/pi.js";

const IDENT = { nonce: "n0", policy_digest: "sha256:p" };
// A reply from the identified daemon, bound to the envelope it was sent.
const decisionReply =
  (verdict: string, mode = "enforce", nonce = IDENT.nonce) =>
  async (_url: unknown, init?: { body?: unknown }) =>
    new Response(
      JSON.stringify({
        decision: {
          verdict,
          mode,
          decision_id: "d_1",
          policy_digest: IDENT.policy_digest,
          envelope_digest: digest(JSON.parse(String(init?.body ?? "{}"))),
        },
        packet: `cacogate ▸ ${verdict}`,
      }),
      {
        status: 200,
        headers: { "content-type": "application/json", "x-cacogate-daemon": nonce },
      },
    );

beforeEach(() => {
  // Every test runs against a published identity in its own data dir.
  const dir = mkdtempSync(join(tmpdir(), "cacogate-test-"));
  writeFileSync(join(dir, "daemon.json"), JSON.stringify({ ...IDENT, pid: 1 }));
  vi.stubEnv("CACOGATE_DATA_DIR", dir);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
});

describe("AD001 enforcement mapping matches Rust", () => {
  it("shadow never interferes", () => {
    for (const v of [
      "allow",
      "sandbox",
      "canary",
      "revise",
      "request_evidence",
      "ask_human",
      "block",
    ] as const) {
      expect(enforcement(v, "shadow")).toBe("defer");
    }
  });
  it("held verdicts never defer in enforce mode", () => {
    for (const v of ["revise", "request_evidence", "ask_human", "block"] as const) {
      expect(enforcement(v, "enforce")).not.toBe("defer");
    }
    expect(enforcement("allow", "enforce")).toBe("defer");
  });
});

describe("envelope", () => {
  it("digest binds tool, args, cwd and root", () => {
    const e = buildEnvelope({
      harness: "pi",
      sessionId: "s",
      toolUseId: "t",
      tool: "bash",
      args: { command: "ls" },
      cwd: "/repo",
      repo: { root: "/repo" },
    });
    expect(e.action_digest).toBe(actionDigest("bash", { command: "ls" }, "/repo", "/repo"));
    expect(e.action_digest).not.toBe(
      actionDigest("bash", { command: "rm -rf /" }, "/repo", "/repo"),
    );
  });
});

describe("fail-safe fallback when the daemon is down", () => {
  const env = (tool: string) =>
    buildEnvelope({
      harness: "opencode",
      sessionId: "s",
      toolUseId: "t",
      tool,
      args: {},
      cwd: "/tmp",
      repo: { root: "/tmp" },
    });
  const down = (() => Promise.reject(new Error("ECONNREFUSED"))) as unknown as typeof fetch;

  it("read tools pass, others are refused in enforce", async () => {
    expect((await checkAction(env("read"), { mode: "enforce", fetchImpl: down })).enforcement).toBe(
      "defer",
    );
    const o = await checkAction(env("bash"), { mode: "enforce", fetchImpl: down });
    expect(o.enforcement).toBe("deny");
    expect(o.packet).toContain("PRV-GATE-001");
  });
  it("shadow never interferes even when down", async () => {
    expect((await checkAction(env("bash"), { mode: "shadow", fetchImpl: down })).enforcement).toBe(
      "defer",
    );
  });
});

describe("OpenCode plugin", () => {
  it("throws the packet on deny and passes on allow", async () => {
    const hooks = await CacogatePlugin({ directory: "/tmp", worktree: "/tmp" });
    vi.stubGlobal("fetch", vi.fn(decisionReply("block")));
    await expect(
      hooks["tool.execute.before"](
        { tool: "bash", sessionID: "s", callID: "c" },
        { args: { command: "x" } },
      ),
    ).rejects.toThrow("cacogate ▸ block");
    vi.stubGlobal("fetch", vi.fn(decisionReply("allow")));
    await expect(
      hooks["tool.execute.before"]({ tool: "read", sessionID: "s", callID: "c" }, { args: {} }),
    ).resolves.toBeUndefined();
  });
  it("sends the exact args it was given", async () => {
    const hooks = await CacogatePlugin({ directory: "/tmp", worktree: "/tmp" });
    const f = vi.fn(decisionReply("allow"));
    vi.stubGlobal("fetch", f);
    await hooks["tool.execute.before"](
      { tool: "bash", sessionID: "s", callID: "c" },
      { args: { command: "git push" } },
    );
    const body = JSON.parse((f.mock.calls[0] as unknown as [string, { body: string }])[1].body);
    expect(body.args).toEqual({ command: "git push" });
    expect(body.harness.name).toBe("opencode");
  });
});

describe("pi extension", () => {
  it("tells omp from pi by process title, then argv", () => {
    expect(harnessName("omp", ["/usr/bin/bun", "/x/cli.ts"])).toBe("omp");
    expect(harnessName("bun", ["/usr/bin/bun", "/h/pi-builds/oh-my-pi/src/cli.ts"])).toBe("omp");
    expect(harnessName("pi", ["/usr/bin/node", "/h/pi-builds/pi/dist/cli.js"])).toBe("pi");
    expect(harnessName("node", ["/usr/bin/node", "/h/compomp-notes/cli.js"])).toBe("pi");
  });

  type H = (e: unknown, ctx: unknown) => Promise<unknown>;
  const load = () => {
    const handlers: Record<string, H> = {};
    piExtension({
      on: (ev: string, h: H) => {
        handlers[ev] = h;
      },
    } as never);
    return handlers;
  };
  const ctx = (confirm?: boolean) => ({
    cwd: "/tmp",
    hasUI: confirm !== undefined,
    ui: { confirm: async () => confirm ?? false },
    sessionManager: { getSessionId: () => "pi-s" },
  });

  it("blocks on block, asks on ask_human", async () => {
    const h = load();
    vi.stubGlobal("fetch", vi.fn(decisionReply("block")));
    expect(
      await h.tool_call?.({ toolCallId: "1", toolName: "bash", input: { command: "x" } }, ctx()),
    ).toMatchObject({ block: true });
    vi.stubGlobal("fetch", vi.fn(decisionReply("ask_human")));
    expect(
      await h.tool_call?.({ toolCallId: "2", toolName: "bash", input: {} }, ctx(true)),
    ).toBeUndefined();
    expect(
      await h.tool_call?.({ toolCallId: "3", toolName: "bash", input: {} }, ctx(false)),
    ).toMatchObject({ block: true });
    expect(
      await h.tool_call?.({ toolCallId: "4", toolName: "bash", input: {} }, ctx()),
    ).toMatchObject({ block: true });
  });
});

describe("AD017 the TS client trusts only the identified daemon", () => {
  const env = buildEnvelope({
    harness: "pi",
    sessionId: "s",
    toolUseId: "t",
    tool: "bash",
    args: { command: "git push --force origin main" },
    cwd: "/tmp",
  });
  const opts = { mode: "enforce" as const };

  it("uses the verdict when nonce, policy and envelope all match", async () => {
    const o = await checkAction(env, { ...opts, fetchImpl: decisionReply("allow") as never });
    expect(o.verdict).toBe("allow");
    expect(o.degraded).toBeUndefined();
  });

  it("discards a well-formed allow from anything it cannot identify", async () => {
    const unbound = async () =>
      new Response(
        JSON.stringify({
          decision: {
            verdict: "allow",
            decision_id: "d",
            policy_digest: IDENT.policy_digest,
            envelope_digest: "sha256:other",
          },
          packet: "p",
        }),
        { status: 200, headers: { "x-cacogate-daemon": IDENT.nonce } },
      );
    const cases: [string, Parameters<typeof checkAction>[1]][] = [
      ["wrong nonce", { ...opts, fetchImpl: decisionReply("allow", "enforce", "x") as never }],
      ["no identity", { ...opts, identity: null, fetchImpl: decisionReply("allow") as never }],
      ["unbound reply", { ...opts, fetchImpl: unbound as never }],
      [
        "other policy",
        {
          ...opts,
          identity: { nonce: IDENT.nonce, policy_digest: "sha256:q" },
          fetchImpl: decisionReply("allow") as never,
        },
      ],
    ];
    for (const [name, o] of cases) {
      const r = await checkAction(env, o);
      expect(r.verdict, name).toBe("block");
      expect(r.enforcement, name).toBe("deny");
      expect(r.degraded, name).toMatch(/identity mismatch|unbound/);
    }
  });
});

describe("AD018 adapters record the model that proposed each action", () => {
  const sent = () => {
    const bodies: Record<string, unknown>[] = [];
    const f = vi.fn(async (url: unknown, init?: { body?: unknown }) => {
      bodies.push(JSON.parse(String(init?.body ?? "{}")) as Record<string, unknown>);
      return decisionReply("allow")(url, init);
    });
    vi.stubGlobal("fetch", f);
    return bodies;
  };

  it("pi/omp: ctx.model becomes envelope.model", async () => {
    const bodies = sent();
    const handlers: Record<string, (e: unknown, c: unknown) => Promise<unknown>> = {};
    piExtension({
      on: (ev: string, h: never) => {
        handlers[ev] = h;
      },
    } as never);
    await handlers.tool_call?.(
      { toolCallId: "1", toolName: "bash", input: { command: "ls" } },
      { cwd: "/tmp", model: { id: "gx10-35b", provider: "llamacpp" } },
    );
    expect(bodies[0]?.model).toEqual({ id: "gx10-35b", endpoint_kind: "llamacpp" });
  });

  it("OpenCode: the model from chat.params is attached to later tool calls", async () => {
    const bodies = sent();
    const hooks = (await CacogatePlugin({
      directory: "/tmp",
      worktree: "/tmp",
    })) as unknown as Record<string, (i: unknown, o: unknown) => Promise<unknown>>;
    await hooks["chat.params"]?.(
      { sessionID: "s", model: { id: "qwen3.6", providerID: "ollama" } },
      {},
    );
    await hooks["tool.execute.before"]?.(
      { tool: "bash", sessionID: "s", callID: "c" },
      { args: { command: "ls" } },
    );
    expect(bodies[0]?.model).toEqual({ id: "qwen3.6", endpoint_kind: "ollama" });
  });
});
