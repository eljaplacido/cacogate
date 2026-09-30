// ActionEnvelope construction (contracts/v0/action-envelope.schema.json).
// Adapters send the raw action only; every risk input is computed by the gate.
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, isAbsolute, join } from "node:path";
import { digest } from "./jcs.js";

export const CONTRACT_VERSION = "0.1.0";
export const ADAPTER_VERSION = "0.0.1";

export type HarnessName = "opencode" | "pi" | "omp" | "gnomon" | "other";

export interface Repo {
  root?: string;
  head?: string;
  branch?: string;
}

/** Model identity recorded with each action (a decision driver). */
export interface Model {
  id?: string;
  endpoint_kind?: string;
}

/**
 * Version of the package that owns a CLI entry point: walk up from the entry
 * script to the nearest package.json whose name contains `nameHint`.
 */
export function packageVersionNear(
  entry: string | undefined,
  nameHint: string,
): string | undefined {
  if (!entry) return undefined;
  let dir = dirname(entry);
  for (let i = 0; i < 5; i++) {
    try {
      const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8")) as {
        name?: string;
        version?: string;
      };
      if (typeof pkg.version === "string" && (pkg.name ?? "").includes(nameHint))
        return pkg.version;
    } catch {
      // not here; keep walking
    }
    const up = dirname(dir);
    if (up === dir) break;
    dir = up;
  }
  return undefined;
}

export interface ActionEnvelope {
  contract_version: string;
  envelope_id: string;
  harness: { name: HarnessName; version: string; adapter_version: string; interactive?: boolean };
  session_id: string;
  model?: Model;
  tool_use_id: string;
  tool: { raw_name: string };
  args: Record<string, unknown>;
  cwd: string;
  repo?: Repo;
  ts: string;
  action_digest: string;
}

/** Same identity as Rust `action_digest`: tool, args, cwd, root (null when absent). */
export function actionDigest(
  tool: string,
  args: Record<string, unknown>,
  cwd: string,
  root: string | undefined,
): string {
  return digest({ tool, args, cwd, root: root ?? null });
}

/** Walk up to `.git`, read HEAD. Mirrors crates/cacogate-harness/src/repo.rs. */
export function probeRepo(cwd: string): Repo | undefined {
  let dir = cwd;
  for (;;) {
    const dotgit = join(dir, ".git");
    if (existsSync(dotgit)) {
      let gitdir = dotgit;
      try {
        if (!statSync(dotgit).isDirectory()) {
          const p = readFileSync(dotgit, "utf8")
            .trim()
            .replace(/^gitdir: /, "");
          gitdir = isAbsolute(p) ? p : join(dir, p);
        }
        const head = readFileSync(join(gitdir, "HEAD"), "utf8").trim();
        if (head.startsWith("ref: ")) {
          const ref = head.slice(5);
          const repo: Repo = { root: dir, branch: ref.replace(/^refs\/heads\//, "") };
          const refPath = join(gitdir, ref);
          if (existsSync(refPath)) repo.head = readFileSync(refPath, "utf8").trim();
          return repo;
        }
        return { root: dir, head };
      } catch {
        return { root: dir };
      }
    }
    const parent = dirname(dir);
    if (parent === dir) return undefined;
    dir = parent;
  }
}

export function buildEnvelope(input: {
  harness: HarnessName;
  harnessVersion?: string;
  model?: Model | undefined;
  interactive?: boolean;
  sessionId: string;
  toolUseId: string;
  tool: string;
  args: Record<string, unknown>;
  cwd: string;
  repo?: Repo | undefined;
  now?: Date;
}): ActionEnvelope {
  const repo = input.repo ?? probeRepo(input.cwd);
  const env: ActionEnvelope = {
    contract_version: CONTRACT_VERSION,
    envelope_id: `env_${input.toolUseId}`,
    harness: {
      name: input.harness,
      version: input.harnessVersion ?? "unknown",
      adapter_version: ADAPTER_VERSION,
      ...(input.interactive === undefined ? {} : { interactive: input.interactive }),
    },
    session_id: input.sessionId,
    tool_use_id: input.toolUseId,
    tool: { raw_name: input.tool },
    args: input.args,
    cwd: input.cwd,
    ts: (input.now ?? new Date()).toISOString(),
    action_digest: actionDigest(input.tool, input.args, input.cwd, repo?.root),
  };
  if (repo) env.repo = repo;
  if (input.model && (input.model.id || input.model.endpoint_kind)) env.model = input.model;
  return env;
}
