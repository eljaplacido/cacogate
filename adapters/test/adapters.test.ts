import { afterEach, describe, expect, it, vi } from "vitest";
import { checkAction, enforcement } from "../src/core/client.js";
import { actionDigest, buildEnvelope } from "../src/core/envelope.js";
import { CacogatePlugin } from "../src/opencode.js";
import piExtension, { harnessName } from "../src/pi.js";

const decisionReply = (verdict: string, mode = "enforce") =>
  new Response(
    JSON.stringify({
      decision: { verdict, mode, decision_id: "d_1" },
      packet: `cacogate ▸ ${verdict}`,
    }),
    { status: 200, headers: { "content-type": "application/json" } },
  );

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
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => decisionReply("block")),
    );
    await expect(
      hooks["tool.execute.before"](
        { tool: "bash", sessionID: "s", callID: "c" },
        { args: { command: "x" } },
      ),
    ).rejects.toThrow("cacogate ▸ block");
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => decisionReply("allow")),
    );
    await expect(
      hooks["tool.execute.before"]({ tool: "read", sessionID: "s", callID: "c" }, { args: {} }),
    ).resolves.toBeUndefined();
  });
  it("sends the exact args it was given", async () => {
    const hooks = await CacogatePlugin({ directory: "/tmp", worktree: "/tmp" });
    const f = vi.fn(async () => decisionReply("allow"));
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
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => decisionReply("block")),
    );
    expect(
      await h.tool_call?.({ toolCallId: "1", toolName: "bash", input: { command: "x" } }, ctx()),
    ).toMatchObject({ block: true });
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => decisionReply("ask_human")),
    );
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
