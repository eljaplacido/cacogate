# cacogate — Development Blueprint

*Version 0.1 (2026-09-26). Companion to `00-SYNTHESIS.md`; section references like "S§4.3" point into that document. `[IP]` marks components relevant to the patent claim; keep them private until disclosure (S§9).*

---

## 0. One-paragraph definition

**cacogate** is a local, deny-first **assurance daemon** that every coding-agent harness asks *before* it executes a tool action.

For each action it computes four independently reported checks:
- **symbolic**: policy invariants, including rules over the session's history;
- **causal**: which parts of the repository the action can affect, and whether the effect is identifiable;
- **conformal**: calibrated false-allow risk per execution mode;
- **budget**: session and agent risk budgets.

It combines them on a monotone **verdict ladder**:

```
allow < sandbox < canary < revise < request_evidence < ask_human < block
```

It then answers the harness in that harness's native protocol. Every allowed or sandboxed action gets an **evidence-bound, single-use capability** `[IP]`. Every decision, approval, execution and realised outcome is recorded in a tamper-evident ledger. The same ledger supplies the labels that calibrate the gate, closing the loop from each decision to its realised outcome.

**Non-goals (v0).**
- It is not an LLM judge.
- It does not replace SeptaCore, which gates the tree in CI.
- It does not replace TrustLayer, which remains the fleet-wide evidence and compliance store.
- It does not replace native harness permissions. It only tightens them: an `allow` verdict never grants more than the harness would have allowed on its own.
- It does not claim legal compliance.

---

## 1. Architecture

### 1.1 Tiered evaluation (hot path vs slow path)

```
harness adapter ──(ActionEnvelope)──► daemon :4320 / unix socket
                                        │
  T0  normalise + facts (pure)  ≤ 2 ms  │ tool kind, parsed shell AST, paths, destinations,
                                        │ reversibility, privilege, secret-touch, self-protection
  T1  symbolic kernel (pure)    ≤ 2 ms  │ invariant / defeasible / advisory rules + session-state monitor
  T2  causal impact (cached)    ≤ 10 ms │ Repository Impact Graph (RIG) descendants → blast radius
  T3  conformal apply (pure)    ≤ 1 ms  │ frozen CalibrationArtifact → permitted-mode set
  T4  budget + ladder (pure)    ≤ 1 ms  │ weakest-allowed verdict, capability mint
        ── p95 budget for T0–T4: 15 ms in-daemon, 30 ms hook round-trip ──
  T5  SLOW PATH (only if verdict ∈ {request_evidence, ask_human} or action class irreversible,
      and policy permits): shadow execution in sandboxed worktree, typed-question features
      (local S1 / Jev), causal-core estimation, `septacore check`. Bounded by deadline;
      deadline miss ⇒ degradation record + stay at current (stricter) verdict.
```

T0–T4 are **pure functions** over `(envelope, session_state, policy, rig_snapshot, calibration)`. The imperative shell is the daemon: I/O, clock, sockets, the ledger writer. This is the causalplatform ledger style (the caller supplies the clock) and keeps the trusted computing base small (RQ6).

### 1.2 Components

| Component | Plane | Responsibility |
|---|---|---|
| **Adapters** | decision↔assurance | Translate each harness's hook/plugin payload ⇄ `ActionEnvelope` / `HarnessResponse`. Thin, dependency-free. |
| **Facts extractor** | assurance | Deterministic `ActionFacts`. Bash is parsed with tree-sitter-bash into simple commands and matched against a **command-effect catalogue**. Unparseable ⇒ `could_not_measure`. |
| **Symbolic kernel** | assurance | Rules compiled from `policies/*.toml`. Adds a stateful monitor over the session's event history ("no `git push` unless tests passed after the last edit", "no approval by the proposing agent"). |
| **Causal impact** | assurance | **Repository Impact Graph (RIG)**: nodes are files, modules, tests, packages, services, deploy targets and external resources; edges are imports, coverage (tests that cover), build dependencies, deploy mapping, CODEOWNERS. `do(action)` ⇒ descendant set ⇒ blast radius. The slow path runs interventional estimates over process variables using `causal-core`. |
| **Conformal engine** `[IP]` | assurance | Applies a signed `CalibrationArtifact`: per-stratum, per-mode thresholds λ̂ₘ from conformal risk control. Online ACI update and an exchangeability martingale for detecting distribution shift. |
| **Ladder + capability** `[IP]` | assurance | Weakest-allowed combination. Mints `Capability` = HMAC(action_digest, mode, decision_digest, ttl, nonce), consumed once. |
| **Sandbox backends** | assurance | `bwrap` (default on Linux), `docker --network none`, `nemoclaw/openshell` (after onboarding). Each **declares** what it confines (fs / network / credentials / process) and self-tests at startup. |
| **Ledger** | evidence | Append-only JSONL hash chain per session, a SQLite index (`~/.local/share/cacogate/ledger.sqlite`; **never** named `brain.db`) and Ed25519 checkpoints. Exporters: TrustLayer `/v1/events`, OTel GenAI spans (`execute_tool` + `cacogate.*` attributes), W3C PROV bundles, `cb-gx10 log` milestones. |
| **Outcome labeller** | evidence | Joins each decision to its realised outcome: PostToolUse exit bucket, subsequent test/verify results, reverts within N turns or commits, CI status, human post-hoc labels. Emits `OutcomeRecord`s, which are the calibration data. |
| **Lab** (Python, offline) | R&D | Fits scorers, runs CRC calibration, runs causal studies with `causal-core`, runs the Jev/S1 bake-off, runs benchmarks, writes signed artefacts. **Never on the hot path.** |
| **Surfaces** | UX | Inline harness packets, the `cacogate` CLI and TUI, and a local web app served by the daemon. |

### 1.3 Why this shape

- **One daemon, many thin adapters.** Latency (S§4.5), a single policy/calibration state, and a consistent ledger.
- **Pure kernel.** Property-testable. The TCB is enumerable. It is the same code in the daemon, the CLI replay and the conformance runner.
- **Lab/kernel split via signed artefacts.** Research velocity (Python, causal-core, DoWhy, MAPIE-style experiments) without Python on the hot path. This is the Oracle type wall made structural: the lab can only influence the kernel through a versioned, signed, expiring artefact.

---

## 2. Verdict semantics

### 2.1 The ladder

| Rung | Verdict | Executes? | Meaning |
|---|---|---|---|
| 0 | `allow` | yes, natively | Defer to the harness's own permission flow. cacogate adds nothing, and never grants more than the harness already would. |
| 1 | `sandbox` | yes, confined | Rewritten to `cacogate exec --cap <token> -- <cmd>` on a declared backend. |
| 2 | `canary` | yes, limited + watched | Deploy/publish-class actions: a limited target plus a watched follow-up (causal-core anytime-valid sequences). |
| 3 | `revise` | no | Returned to the agent with machine-readable constraints ("stay within `src/api/`", "use `--dry-run` first"). |
| 4 | `request_evidence` | no, until the obligations are met | Named obligations, e.g. `run: cargo test -p x`, `septacore check == 0`, `shadow_run`. The retry passes when the ledger shows them satisfied **after** the action's pre-image. |
| 5 | `ask_human` | only after approval | Native prompt when the harness is interactive; otherwise queued to the TUI/web app. The approval is bound to `action_digest`. |
| 6 | `block` | never | Invariant violation. It cannot be overridden in-session; only a policy change can lift it, and that change is itself gated. |

### 2.2 Combination rule (monotone)

- Every `Check` names `allows` = the **least restrictive** rung it still permits.
- The decision is the **most restrictive** of all `allows` (causalplatform `decide()` semantics).
- Adding a check can only lower autonomy; it can never raise it.
- Property test: for all check multisets C and all checks c, `rung(C ∪ {c}) ≥ rung(C)`.

### 2.3 Check status is four-valued

- `passed / failed / not_applicable / could_not_measure` (SeptaCore DG-7).
- A `could_not_measure` check contributes its declared `allows_when_unmeasured`: by default `ask_human` for mutating classes and `allow` for read-only classes.
- `not_applicable` contributes nothing, but if *every* check is `not_applicable` ⇒ `could_not_measure` overall (DG-5).

### 2.4 Rule classes (RQ5)

| Class | Max verdict | Override |
|---|---|---|
| `invariant` | `block` | None in-session. |
| `defeasible` | ≤ `ask_human` | Human override with a reason of at least 20 characters; the override record is chained. |
| `advisory` | ≤ `revise` | The agent may proceed after acknowledging; only the reason is shown to it. |

Precedence is fixed: `invariant > defeasible > advisory`. Conflicts inside a class resolve to the more restrictive rule.

### 2.5 Fail semantics (gate faults)

| Action class | Daemon unreachable / timeout / internal fault |
|---|---|
| read-only (read, grep, glob, ls) | `allow` + `degradation` record (configurable) |
| local mutating (edit, write, bash in root) | `ask_human` |
| irreversible / external (push, publish, deploy, network write, outside-root, credentials) | `block` |

Implementation detail: adapters embed the minimal static class table so they can fail correctly without the daemon, and they can deny with exit 2.

---

## 3. The pillars in detail

### 3.1 Symbolic kernel

**Policy format.** A TOML policy pack, validated against `contracts/v0/policy.schema.json`. Unknown keys are rejected (SeptaCore DG-21).

```toml
[[rule]]
id = "INV-SELF-001"                      # self-protection
class = "invariant"
when.tool_kind = ["file_write", "file_edit", "shell"]
when.touches_any = ["~/.claude/settings*.json", "**/.opencode/plugin/**", "~/.pi/agent/extensions/**",
                    "**/.gnomon/policy.toml", "~/.config/cacogate/**"]
allows = "block"
reason = "Agents may not modify their own gate or hook configuration."

[[rule]]
id = "DEF-VCS-003"
class = "defeasible"
when.shell_command = { program = "git", subcommand = "push" }
when.session.not_since_last_edit = { evidence = "tests_passed" }  # temporal
allows = "request_evidence"
obligations = [{ kind = "run", match = "test_command" }]
```

**Predicates.**
- The TrustLayer operator set: `$eq $ne $in $nin $gt $gte $lt $lte $exists $contains $prefix $suffix`, plus path globs.
- Matching runs over **parsed argv tokens**. There is no regex on raw command strings, which both avoids denial of service and closes the redirection and substitution bypasses gnomon had to special-case.

**Temporal monitor.**
- A small past-time LTL fragment (`since`, `once`, `previously`) evaluated incrementally over the session event stream.
- This is TrustLayer's missing Phase 9.

**Built-in default policy (strict).**
- It applies when the policy is absent or invalid, and the daemon logs at CRITICAL (no allow-all; S§5).
- It includes: force-push or protected-branch pushes; `rm -rf` outside the root; writes outside the root; `curl | sh`; credential files; `sudo`; `chmod -R 777`; package publish; cloud CLI mutations (`kubectl apply`, `terraform apply`, `az`, `gcloud … create|delete`); disabling hooks.

**Optional Rego** via `regorus` behind a cargo feature, for users who already have OPA policies.

### 3.2 Causal impact

**RIG construction** (`cacogate rig build`, also incremental on file change):
- Language-aware import graphs: tree-sitter for TS/Python/Rust/Go, plus `cargo metadata` and `pnpm` workspace graphs.
- Test→code coverage mapping, taken from the last coverage run when one exists, otherwise from name and import heuristics. Heuristic edges are flagged `origin=heuristic`, following causalplatform's `EdgeOrigin`.
- Deploy mapping parsed from CI workflows, Dockerfiles and k8s/compose manifests.
- The graph is content-addressed (`rig_digest`), so each decision can cite the exact graph it used.

**Structural query (hot path).**
- `do(edit F)` ⇒ descendants D(F), tests covering D, deploy targets reachable, CODEOWNERS crossed.
- Blast radius = the vector `(|D|, |tests|, services, public_api_touched, owners_crossed)`. It is used for rule predicates and as scorer features, **never as a probability**.

**Interventional estimation (lab / slow path).**
- A process-level DAG over session variables: task class, model, harness, file centrality, tests run before the action, verify enabled, … → outcome.
- Estimated with `causal-core` on ledger history, with explicit identification status. Its uses:
  1. choose *which obligation* `request_evidence` asks for, meaning the intervention with the best estimated harm reduction per unit of cost;
  2. produce `what_would_change_it` text grounded in estimated effects;
  3. map `Abstention` ⇒ `CausalCheck.status = could_not_measure`, with `missing_information`, which caps the ladder at `request_evidence` (RQ1).

**Counterfactual shadow twin.**
- On the slow path, run the action in a disposable worktree on a sandbox backend and diff the observed effects (files changed, tests, network attempts) against the RIG's prediction.
- A mismatch is an escalation signal *and* a counterfactual outcome label, which addresses the MNAR problem (S§4.4).

### 3.3 Conformal engine `[IP]`

**Setting.**
- The stratum k is the action class: `read / local_edit / shell_local / vcs_local / vcs_remote / network / deploy / mcp`. Mondrian strata are kept coarse because n is small.
- Execution modes: `M = {direct, sandbox, canary}` (canary applies to deploy strata only).
- Frozen scorer: s(x) ∈ ℝ, fitted on split A of the ledger. Features come from `ActionFacts`, the RIG blast radius, session history and optional typed-question features.
- Loss for mode m: ℓₘ(x, y) ∈ [0,1]. It is 1 if executing x in mode m led, or would have led, to a harmful outcome y. Harmful means reverted, tests broken and not fixed within the session, CI failed, destructive side effect, a canary tripped, or a human post-hoc "should not have run".

**Calibration (split B, per stratum k and mode m).**
- Lᵢ(λ) = ℓₘ(xᵢ, yᵢ) · 𝟙[s(xᵢ) ≤ λ], which is non-decreasing in λ.
- λ̂ₘ,ₖ = sup{ λ : (n/(n+1))·R̂ₙ(λ) + 1/(n+1) ≤ αₘ,ₖ }  (conformal risk control, B = 1).
- Guarantee, under exchangeability within the stratum: E[false-allow loss] ≤ αₘ,ₖ. This is **marginal** over the stratum, not per action. The packet says so.

**Runtime.**
- Permitted modes P(x) = { m : s(x) ≤ λ̂ₘ,ₖ }. The conformal check allows the least restrictive mode in P(x).
- P(x) = ∅ ⇒ `allows = ask_human`.

**Status values** (from the artefact and the online monitors):
- `calibrated`
- `insufficient_n`: n below the preregistered n_min, default 100. Resolves to `could_not_measure` (S§4.3).
- `stale`: an artefact binding digest doesn't match the current harness / model / policy / scorer / toolset (RQ8).
- `shifted`: the exchangeability martingale crossed 1/δ. Mutating strata are capped at `ask_human` until recalibration.

**Online adaptation.**
- ACI (Gibbs–Candès), αₜ₊₁ = αₜ + γ(α − errₜ), on newly labelled outcomes, with γ preregistered.
- Weighted CP with likelihood-ratio or SCM-derived weights is the research extension (phdresearch E1). It stays in the lab until it beats split CRC on the preregistered shift benchmark.

**Reporting discipline (S§4.1).**
- α is always labelled `target`.
- The measured held-out false-allow rate is reported with a Clopper-Pearson CI, and n per stratum is always shown.

### 3.4 Budget check

- Each session and agent has a risk budget β (preregistered).
- Each auto-executed action spends its upper-bound risk: the stratum's α for the mode used.
- When β is exhausted ⇒ `allows = ask_human` for the remainder. A human approval refunds nothing; it resets the budget with a record.
- Also: ask-rate budgeting for human attention (RQ9). When the ask queue exceeds its bound, stack the pending asks into one batched review instead of silently allowing them.

### 3.5 Typed-question features (local S1 / Jev)

- `FeatureProvider` interface: `ask(questions: TypedQuestion[], state: RedactedState) -> Answers`.
- Question types mirror Jev: `noul / choice / score`. **Every question includes escape options** (`insufficient_evidence`, `out_of_scope`, `requires_intervention_analysis`); the audit shows ECE goes from 0.023 to 0.793 without them.
- Providers: `local-s1`, a grammar-constrained Ollama model with logprobs and temperature scaling (qwen3.6:35b, nemotron-3-nano:4b for speed), and `jev` (pinned `jev-1.13.0`, `TYPESAFE_API_KEY`, `sensitivity=public` only).
- **Type wall.** `Answers` can only flow into scorer features, enforced by the type system (Rust newtype plus the Python mypy test from Oracle). Nothing in the ladder accepts a probability.
- Enabling a question in production requires a passed bake-off artefact (benchmark B6).

### 3.6 Evidence-bound capability `[IP]`

- `Capability { cap_id, action_digest, mode, decision_digest, not_after, nonce }` + HMAC-SHA256 over its JCS form.
- The key lives in `~/.config/cacogate/keys/` (0600). Rotation is supported. The daemon **refuses to start without a key** outside `--dev`.
- It is consumed once: the consumption table's primary key is `cap_id`.
- `cacogate exec` and `cacogate run` (the wrappers used for sandbox, canary and deploy) verify the capability and **recompute `action_digest` from the actual argv**, so approving X can never authorise Y (causalplatform's closed `guardian-verdict-binding` bug).

---

## 4. Contracts

All contracts are **JSON Schema 2020-12** in `contracts/v0/` and are the single source of truth.
- Every object has `additionalProperties: false`.
- Every contract is versioned (`contract_version`).
- Digests are sha256 over **RFC 8785 JCS**, so Rust, Python and TS produce identical bytes. This is pinned by `conformance/vectors/digest_vectors.json`.

| Contract | Key fields | Producer → consumer |
|---|---|---|
| `ActionEnvelope` | `envelope_id, contract_version, harness{name,version,adapter_version}, session_id, turn_id?, tool_use_id, agent{role, parent_id?}, model{id, endpoint_kind}?, tool{raw_name, kind}, args (canonical), cwd, repo{root, head, dirty}, surface_digest?, ts, action_digest` | adapter → daemon |
| `ActionFacts` | `kind, commands[] (argv, program, subcommand, flags, redirects, pipes), paths_read[], paths_written[], outside_root, destinations[], reversibility (reversible\|compensable\|irreversible), privilege, secret_touch, self_protection_hit, parse_status` | kernel internal (recorded) |
| `Check` | `pillar (symbolic\|causal\|conformal\|budget\|provenance), id, status (4-valued), allows (Verdict), detail, evidence_refs[], rule_class?` | kernel → decision |
| `GateDecision` | `decision_id, envelope_digest, verdict, checks[], obligations[], constraints[], rationale (templated, with claim ids), what_would_change_it[], risk{conformal{stratum, mode_set, alpha_target, status, calibration_id, n_cal}, causal{blast_radius, identification, rig_digest}}, policy_digest, calibration_digest?, capability?, degradations[], latency_us, decision_digest` | daemon → adapter/ledger |
| `HarnessResponse` | per-adapter projection of a `GateDecision` (see §5) | daemon → adapter |
| `HumanDecision` | `decision_id, choice (approve\|reject\|modify), reviewer_id, rationale, modified_args?, surface (terminal\|tui\|web\|chat), packet_variant, latency_ms` | surface → ledger |
| `ExecutionRecord` | `decision_id, cap_id?, executed (bool), mode, exit_code, bucket (result\|refusal\|apparatus_failure), duration_ms` | adapter/`cacogate exec` → ledger |
| `OutcomeRecord` | `decision_id, label (benign\|harmful\|undecidable), label_source (auto:revert\|auto:tests\|auto:ci\|shadow\|human), harm_kind?, observed_at, counterfactual (bool)` | labeller → ledger → lab |
| `CalibrationArtifact` | `calibration_id, method (split_crc\|aci\|weighted_crc), scorer_digest, strata{k:{mode:{alpha_target, lambda_hat, n, measured_false_allow, ci}}}, binds{harness, model, policy, toolset, scorer}, data_range{ledger_from, ledger_to, digest}, prereg_sha256, created_at, expires_at, signature` | lab → daemon |
| `LedgerEntry` | `seq, session_id, kind (envelope\|decision\|human\|execution\|outcome\|degradation\|checkpoint), body_digest, body?, prev_hash, hash, recorded_at` | daemon |
| `Checkpoint` | `session_id, seq, head_hash, created_at, public_key, signature` (TrustLayer-compatible) | daemon → anchor sinks |

**Code generation and drift.**
- Rust types are hand-written serde with `deny_unknown_fields`, *checked* against the schema by a round-trip test over golden fixtures.
- Python types are pydantic v2 (`extra="forbid", frozen=True, allow_inf_nan=False`), generated with `datamodel-code-generator` and committed.
- TS types are generated with `json-schema-to-typescript` and committed.
- CI fails if regenerating produces a diff.
- Changing a contract requires a fixture change (a port of gnomon's `contract_fixture_gate.sh`).

Starter schemas are in `contracts/v0/`.

---

## 5. Harness adapters

| Verdict | Claude Code (`PreToolUse`, http hook) | OpenCode plugin | Pi / omp extension | gnomon (native, proposed) | Codex / Gemini | OpenFang |
|---|---|---|---|---|---|---|
| allow | no decision (exit 0, empty) | return | return `{}` | proceed | exit 0 | approve via REST |
| sandbox | `allow` + `updatedInput.command = "cacogate exec --cap … -- <cmd>"` | mutate `output.args.command` | `{input: …}` | `ctx.exec` override for this call | deny + reason (no rewrite) ⇒ **degradation** | reject + reason |
| canary | as sandbox, via the `cacogate run --canary` wrapper | as sandbox | as sandbox | as sandbox | degradation | reject |
| revise | `deny` + reason (constraints as structured text) + `additionalContext` | `throw` with packet | `{block, reason}` | refusal code 2 + detail | deny + reason | reject + reason |
| request_evidence | `deny` + obligations list | `throw` | `{block, reason}` | refusal code 2 + obligations | deny | reject |
| ask_human | `ask` + packet (interactive); headless ⇒ `deny` + queued id | `permission.ask → "ask"` | `ctx.ui.confirm(packet)` | `Approver` (extended request with args) | `ask` (Codex) / deny (Gemini) | leave pending; human approves in the cacogate TUI or web app, which calls the REST API |
| block | `deny` (exit 2 fallback) | `throw` | `{block, reason, terminate?}` | refusal code 3 | deny / exit 2 | reject |
| outcome | `PostToolUse` → `ExecutionRecord` | `tool.execute.after` | `tool_result` event | `tool_call` audit record | PostToolUse | `/api/audit/recent` poll |

**Per-adapter obligations.**
- Every adapter ships a **replay fixture set**: real payloads captured from that harness version, plus the expected responses.
- Every adapter ships a **degradation manifest** of what it cannot enforce, for example Codex `apply_patch` deny (#27833), OpenFang async-only, and Gemini with no ask.
- `cacogate doctor` prints every installed harness, its adapter state and its degradations.

**Packaging.**
- `adapters/claude-code/` is a Claude Code plugin: `hooks.json` (PreToolUse/PostToolUse/SessionStart), the skills from §9 and `/cacogate` commands.
- `adapters/opencode/` is a `.ts` plugin with zero dependencies.
- `adapters/pi/` is a `.ts` extension, compatible with both upstream pi and omp.
- `adapters/codex/` and `adapters/gemini/` are config snippets plus the shared Rust `cacogate hook <harness>` command.
- `adapters/openfang/` is a poller in the daemon.
- `adapters/gnomon/`: see below.

**gnomon native integration** (a separate, one-slice PR to gnomon; it respects "removing cacogate leaves gnomon whole"):
1. A new surface-declared `[gate]` table in `policy.toml` with `url` or `command`, `on_unreachable` and `classes`. It is included in the surface hash.
2. A pre-dispatch call inside `executeTool` **before read-only prefetch**, or prefetch disabled while `[gate]` is set.
3. Verdicts map onto the existing refusal codes 2/3/4. The exit contract is unchanged.
4. A new `AuditKind "gate_decision"` with `CONTRACTS.md` rows, golden fixtures and a negative-control test in `gates.test.ts`.
5. `Approver` gains an optional `args` field, so human asks see real arguments.

**Before native support lands**, use the degraded path: `bash_allow = ['^cacogate run\b']` together with `[verify] command = "cacogate verify"`.

---

## 6. Threat model and TCB

**Assets.** Repository integrity, credentials, remote systems (git remotes, registries, clouds), the gate's own configuration and keys, and ledger integrity.

**Adversaries.**
- A mistaken agent: the default case.
- A prompt-injected agent: via fetched pages, tool output, repository content or memory.
- A strategic agent: rephrasing after a block, splitting a command into innocuous parts, editing the gate's configuration.
- A calibration-gaming actor: poisoning outcome labels.

**Controls, mapped to the OWASP Agentic Top 10 (2026) and MITRE ATLAS** (the table lives in `docs/threat-model.md`):
- Self-protection invariants (INV-SELF-*).
- Parsed-argv matching.
- **Retry-evasion detector**: when an action shares ≥ τ normalised features with a blocked action in the same session, it inherits that block's rung.
- **Composition detector**: a temporal rule for multi-step exfiltration, e.g. `read secret` then `network write`.
- Capabilities bound to the digest.
- Labels accepted only from auto sources or authenticated humans; any label source may be corrupted, so outcome labels are checked against **independent event capture** (harness transcript hash vs ledger).
- Signed policy packs and calibration artefacts (skillvault ATTESTATION format).

**TCB** (enumerated in ADR-0003):
- the `cacogate-kernel` and `cacogate-ledger` crates;
- the capability key;
- the adapter's static fail-class table.

**Explicitly outside the TCB:** the LLM, the lab, the typed-question providers, the web app and the RIG builder. The RIG builder's output is digest-pinned, and a missing RIG ⇒ `could_not_measure`.

---

## 7. Language, typing and tooling

### 7.1 Choices and rationale

| Layer | Language | Why |
|---|---|---|
| Kernel, daemon, CLI, TUI, ledger, sandbox, capability | **Rust** (edition 2024, toolchain pinned to 1.94 as in gnomon) | Hot-path latency without process-spawn cost (S§4.5). A memory-safe TCB. `unsafe` forbidden. It matches causalrust, TrustLayer core-rs and gnomon crates, so their code (e.g. TrustLayer `integrity.rs`, causalrust breaker/rate-limiter) can be reused as path or git dependencies. The binary cold-starts in ~2 ms, so even `command`-type hooks are cheap. |
| Adapters (OpenCode, Pi/omp, gnomon), web app | **TypeScript 7**, ESM, zero runtime dependencies for adapters | The harnesses' own plugin languages. It matches gnomon's "no dependencies" core policy. |
| Lab: calibration, causal studies, bake-offs, benchmarks, reports | **Python 3.12** with **uv** | `causal-core`/`causal-decision` (path-pinned), numpy/scipy, statsmodels, optional MAPIE for cross-checks, and the AgentLab stats conventions (McNemar, Wilson, Clopper-Pearson). |
| Contracts | **JSON Schema 2020-12** + JCS | Language-neutral, with a golden-vector cross-check. |
| Policy | **TOML** (schema-validated), optional Rego | Human-diffable, and it matches `.gnomon/*.toml` and `.septacore.json` habits. |

Considered and rejected:
- **Python kernel**: interpreter start-up and GC jitter on the hot path, and weaker guarantees for the TCB.
- **TS kernel**: it would couple to Node in every harness and does not suit OpenFang or Codex.
- **Go**: no existing portfolio code to reuse.

### 7.2 Rust config

```toml
# Cargo.toml [workspace.lints]
[workspace.lints.rust]
unsafe_code = "forbid"
unreachable_pub = "deny"
missing_docs = "warn"
[workspace.lints.clippy]
correctness = { level = "deny", priority = -1 }
suspicious  = { level = "deny", priority = -1 }
pedantic    = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "deny"
indexing_slicing = "deny"   # causalrust carried 104 of these as debt; start at zero
panic = "deny"
float_cmp = "deny"
```

- `clippy.toml`: `allow-unwrap-in-tests = true`.
- Tools: `cargo fmt --check`, `cargo clippy --all-targets --all-features -D warnings`, `cargo nextest`, `cargo deny` (licenses and advisories), `cargo llvm-cov` (a coverage floor ratchet), `cargo mutants` on the kernel.
- Test libraries: `criterion` benches with latency budgets asserted in CI, `proptest`, `insta` snapshots for packets.
- Crates: `serde` / `serde_json` / `serde_jcs`, `thiserror`, `axum` + `tokio` (daemon only), `clap`, `ratatui`, `rusqlite` (bundled), `ed25519-dalek`, `hmac` + `sha2`, `tree-sitter` + `tree-sitter-bash`, `globset`. The kernel crate itself is **I/O-free**: no tokio, no filesystem.

### 7.3 Python config (`lab/pyproject.toml`)

- Python ≥ 3.12, managed with uv; the lockfile is committed.
- ruff pinned `~=0.15`, line length 100, `select = ["E","F","I","N","W","UP","B","C4","SIM","RUF","PT","S","ANN","PL"]`.
- `mypy --strict` with the `pydantic.mypy` plugin.
- pytest + hypothesis, with markers `statistical`, `network`, `jev`, `slow`. `network` and `jev` are off by default.
- `pip-audit` and `bandit` (via ruff `S`).
- Every pydantic model: `ConfigDict(extra="forbid", frozen=True, allow_inf_nan=False)`.

### 7.4 TypeScript config

- `tsconfig`: `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `verbatimModuleSyntax`, `noImplicitOverride`, `target ES2022`, `module NodeNext`.
- vitest; **biome** for lint and format (one tool; gnomon has none, and this repo starts clean).
- pnpm 9, Node ≥ 20.

### 7.5 Repository gate

- The repo's own gate is **SeptaCore**. `.septacore.json` declares:
  - blocking steps: fmt, clippy, nextest, conformance, schema-drift, ruff, mypy, pytest (unit), vitest, biome, dead-rule test;
  - advisory steps: the latency bench and mutation testing;
  - a ratchet step: coverage.
- Exit codes follow the 0/1/2/3 discipline. GitHub Actions runs the same `septacore check`.
- Conventional commits, one slice per PR, maintainer-gated.
- The honesty register `honesty/declared_not_implemented.json` must list every documented but unbuilt feature. `docs.test` checks README claims against it.

---

## 8. Repository layout

```
cacogate/
├── AGENTS.md                 # constitution for agents working on this repo (CLAUDE.md → @AGENTS.md)
├── README.md
├── docs/
│   ├── 00-SYNTHESIS.md  01-BLUEPRINT.md  threat-model.md  CONTRACTS.md
│   └── adr/0001-…md
├── specs/                    # EARS requirements, IDs GT-*, AD-*, CF-*, LD-*; tests named test_<id>_*
├── contracts/v0/*.schema.json
├── conformance/
│   ├── verdicts.golden.json  check_status.golden.json  fail_classes.golden.json
│   ├── vectors/{digest_vectors,ladder_vectors,policy_vectors,crc_vectors}.json
│   ├── harness/<harness>/<version>/{payloads,expected}/*.json   # replay fixtures
│   └── contract_fixture_gate.sh
├── crates/
│   ├── cacogate-contracts/   # serde types, schema round-trip tests
│   ├── cacogate-kernel/      # PURE: facts, shell parse, symbolic, temporal monitor, rig query,
│   │                         #       conformal apply, budget, ladder, packet render
│   ├── cacogate-ledger/      # chain, checkpoints, sqlite index, exporters (TrustLayer, OTel, PROV)
│   ├── cacogate-sandbox/     # backends (bwrap, docker, nemoclaw) + self-tests
│   ├── cacogate-rig/         # repository impact graph builder (tree-sitter, cargo/pnpm metadata)
│   ├── cacogate-daemon/      # axum http + unix socket, capability mint/consume, labeller, S1 providers
│   └── cacogate-cli/         # `cacogate` binary: hook, exec, run, tui, explain, approve, ledger, doctor
├── adapters/
│   ├── claude-code/          # plugin: hooks.json, commands/, skills/
│   ├── opencode/  pi/  gnomon/  codex/  gemini/
│   └── shared-ts/            # generated contract types + tiny client (no deps)
├── lab/                      # Python uv project `cacogate_lab`
│   └── src/cacogate_lab/{dataset,scorer,crc,aci,shift,causal,features/{local_s1,jev},bench,prereg,report}.py
├── web/                      # Vite + React 18 + TS dashboard, built into the daemon's static assets
├── policies/{default.toml, packs/*}
├── skills/                   # agent skills (§9), with ATTESTATION.json
├── benchmarks/B*/PRE-REGISTRATION.md, results/
├── honesty/{declared_not_implemented.json, readings.jsonl}
└── .septacore.json
```

---

## 9. Agentic skills

Two families of skills. Each is a `SKILL.md` with frontmatter, plus a skillvault-style `ATTESTATION.json`.

**A. Operating skills.** These ship in the Claude Code plugin and are mirrored to `.opencode` and `.pi` skill dirs.

| Skill | Purpose |
|---|---|
| `cacogate-cooperate` | Teaches the coding agent how to read a packet and respond. `revise` ⇒ satisfy the constraints; `request_evidence` ⇒ run the named obligations and then retry the **identical** action; `ask_human` ⇒ stop and summarise for the human; `block` ⇒ do not retry or rephrase (the retry-evasion detector is mentioned explicitly). |
| `cacogate-status` | Shows autonomy budget, calibration status per stratum, pending asks and degradations. |
| `cacogate-explain` | Given a decision id, renders the full packet and evidence subgraph, and answers "why" strictly from ledger fields. Claim ids are bound, following causalplatform `claims.verify`. |
| `cacogate-incident` | Reconstructs a session causally from the ledger: decisions → executions → outcomes. It marks unlogged spans as MNAR gaps. |

**B. Development skills.** These live in this repo's `.claude/skills/`.

| Skill | Purpose |
|---|---|
| `cacogate-spec-first` | EARS requirement → failing test named `test_<id>_…` → implementation → conformance fixture → docs row (the three-point rule). |
| `cacogate-policy-author` | Writes a rule plus its firing fixture and non-firing fixture. Runs `cacogate policy test`. Refuses a rule without fixtures. |
| `cacogate-adapter` | Captures real payloads from a harness version, writes replay fixtures and the degradation manifest, and wires the adapter. |
| `cacogate-calibrate` | Runs lab dataset → scorer → CRC → artefact. Checks `prereg_sha256`, n_min and the binds digests. Signs the artefact. |
| `cacogate-bench` | Runs a preregistered benchmark and writes results with the git SHA and prereg hash. Failing runs are kept. |
| `cacogate-honesty` | Before claiming anything is done, checks `declared_not_implemented.json`, the status claims and the readings ledger. |

---

## 10. UX: terminal-first, with an app for depth

### 10.1 Inline, where the action already blocks (primary)

The `reason`/packet is rendered **within ≤ 6 lines**. The Phase D experiment tunes this format.

```
cacogate ▸ REQUEST EVIDENCE  (rung 4/6)   git push origin feat/x
  why     DEF-VCS-003: no passing test run since last edit (src/api/auth.rs)
  impact  12 dependents · 3 test suites · service api-gateway · owners: @platform
  risk    vcs_remote/direct: n=34 < 100 → uncalibrated (target α=0.05)
  do      run `cargo test -p api` then retry the same command
  id      d_01J…  ·  cacogate explain d_01J…
```

- Packets are deterministic, templated text with claim ids. No LLM prose.
- Colour and glyphs only when the output is a TTY.
- `NO_COLOR` is respected.

### 10.2 `cacogate` CLI and TUI (ratatui)

| Command | Purpose |
|---|---|
| `cacogate watch` | Live decision stream plus the pending-ask queue. Keys: `a` approve (reason required for defeasible rules), `r` reject, `m` modify, `e` expand evidence, `/` filter by harness or session. |
| `cacogate explain <id>` | Renders a decision's packet and evidence. |
| `cacogate approve <id>` / `cacogate reject <id>` | Answers a pending ask; used for headless asks. |
| `cacogate status` | Budgets, calibration and degradations. |
| `cacogate doctor` | Harnesses, adapters, sandbox self-tests and keys. |
| `cacogate ledger verify` | Verifies the ledger chain. |
| `cacogate replay <session>` | Re-runs the kernel over a recorded session; used for policy what-ifs. |
| `cacogate policy test` | Runs policy fixtures. |
| `cacogate calib status` | Shows calibration state. |
| `cacogate rig build` / `cacogate rig show <path>` | Builds or inspects the Repository Impact Graph. |
| `cacogate shadow on\|off` | Shadow mode computes and records verdicts but always answers `allow`. This is how the gate collects calibration data before it enforces anything. |

### 10.3 Web app (`http://127.0.0.1:4320`, over Tailscale on demand)

- **Views**:
  - Approvals inbox, the same queue as the TUI;
  - Decision explorer with an evidence-graph view (requirement → diff → checks → approval → execution → outcome);
  - Calibration: reliability and coverage per stratum with CIs, n, status and shift martingale traces;
  - Autonomy budgets per harness and agent;
  - Policy: rule firing counts and dead-rule warnings;
  - Harness matrix with degradations;
  - **Experiments**: the Phase D packet-variant A/B, with randomised variant assignment logged in `HumanDecision.packet_variant`.
- **Stack**: React 18 + Vite + TS, served as static assets by the daemon. It matches the TrustLayer dashboard. It is read-mostly; approvals go through the same authenticated API as the TUI.
- **Later**: an optional Tauri v2 shell for tray notifications, plus ntfy or Slack for async second-approver requests. The Windows Streamlit dashboard is not replicated; instead, cacogate exports to TrustLayer and `cb-gx10 log`.
- **Ports**: 4320 is chosen to avoid ZeroClaw 4310, OpenFang 4200, TrustLayer 8089 and agentcenter 8765.

---

## 11. Test strategy

| Layer | What | Tooling |
|---|---|---|
| Unit | Every kernel function; the shell-parser corpus (≥ 500 real commands harvested from ledgers and AgentLab diffs) | nextest, pytest, vitest |
| Property | Ladder monotonicity; JCS digest stability; chain verification detects any single-byte edit; capability single-use and digest binding; CRC λ̂ monotone in α; `could_not_measure` never yields `allow` for mutating classes | proptest, hypothesis |
| Conformance | Verdict, status and fail-class enums (the three-point rule); digest, ladder, policy and CRC vectors shared across Rust, Python and TS | golden JSON |
| Negative controls | Every gate can FAIL; **dead-rule test** (every rule has a firing fixture); mutant catalogue (`cargo mutants`, plus named semantic mutants such as "treat unmeasured as pass", "empty policy allows" and "skip temporal monitor") that must be caught | cargo-mutants, custom |
| Adapter replay | Captured payloads for each harness version → expected `HarnessResponse` | fixtures under `conformance/harness/` |
| End-to-end | Scripted headless sessions in scratch repositories for each harness (`claude -p`, `opencode run`, `pi -p`/omp, `gnomon task --json`, `codex exec`) against scenario repos with planted hazards | `benchmarks/e2e/` |
| Red team | gnomon containment and injection harnesses (`benchmarks/results/containment-2026-08*/{runner,scenarios,canary}.py`); AgentDojo / InjecAgent tasks; OWASP-mapped scenarios; retry-evasion and split-command scenarios; calibration-poisoning scenarios. **Each canary has a positive control.** | lab + e2e |
| Performance | T0–T4 p95 ≤ 15 ms; hook round-trip p95 ≤ 30 ms; ≤ 10% of median turn time (AG4) | criterion, e2e timing |
| Statistical | Preregistered benchmarks (§12) | lab |

---

## 12. Benchmark programme (preregistered)

Each benchmark has `benchmarks/B*/PRE-REGISTRATION.md` with a TOML bars block, `n_min`, a stopping rule, a falsifier, and a "what this does not establish" section. Results record `prereg_sha256` and the git SHA. Runs that fail stay on record.

| ID | Question | Primary endpoint | Falsifier |
|---|---|---|---|
| **B0** | Does shadow mode observe everything? | Fraction of harness tool calls with a ledger envelope, per harness | < 99.5% on any harness without a declared degradation |
| **B1** | Is the conformal false-allow rate valid? | Held-out false-allow rate vs α target, per stratum, Clopper-Pearson | Upper CI bound > α + 0.02 at n ≥ n_min |
| **B2** (= AgentLab **AG4**) | Does the gate reduce bad executions at acceptable cost? | blocked-bad, blocked-good, added latency | False-block rate exceeds benefit, or latency > 10% of turn time |
| **B3** | Does it hurt task success? | pass^k and merge-worthy rate (SeptaCore ADR-0008), gate on vs off, paired McNemar, ≥ 3 trials per arm | A significant drop beyond the preregistered margin |
| **B4** | Robustness to shift (RQ2) | Coverage loss and time-to-detect after switching repository, model or harness | Undetected coverage violation lasting > W actions |
| **B5** | Decision-packet experiment (Phase D) | Override correctness, comprehension, response time and fatigue across packet variants | The richer packet shows no improvement over the minimal packet (itself a publishable null) |
| **B6** | Jev / local S1 bake-off | ECE and set size at matched coverage, on mutants with n ≥ 300 and escape options on/off | ECE ≥ 0.05 ⇒ feature not enabled |
| **B7** | Adversarial robustness (RQ12) | Containment rate on injection, evasion and poisoning corpora | Any invariant bypass |
| **B8** | MNAR bias (RQ7) | Calibration bias of executed-only labels vs labels that include shadow counterfactuals | — (exploratory, labelled as such) |

**Where the benchmarks run.**
- Locally on gx10 for B0, B1, B2, B4, B6 and B7.
- In **AgentLab** for B3 and cross-harness B2. Add a `cacogate` wrapper mode to the existing claude_code / opencode / pi / gnomon adapters.
- Results flow back through `cb-gx10 log` and the outbox.

---

## 13. Roadmap with exit criteria

| Phase | Scope | Exit criteria |
|---|---|---|
| **P0 — Evidence first** (week 1–2) | Contracts v0, JCS vectors, ledger, daemon skeleton, **shadow mode**, Claude Code http hook, OpenCode and Pi adapters (observe only), `cacogate watch` (read-only), `doctor`, honesty register. Spikes: OpenCode hook ordering, the Claude Code `defer` semantics, TrustLayer `integrity.rs` reuse, and NemoClaw onboarding vs bwrap. | B0 passes on 3 harnesses; ledger verify is green; latency p95 ≤ 30 ms |
| **P1 — Symbolic** (week 3–4) | Shell parser + effect catalogue, facts, policy TOML, built-in strict default, self-protection, temporal monitor, ladder, packets, enforce mode for **invariants only**, capability + `cacogate exec` with a bwrap sandbox | Dead-rule test and the mutant catalogue are green; B7 has no invariant bypass on the gnomon containment corpus |
| **P2 — Outcomes and conformal** (week 5–8) | Outcome labeller (PostToolUse, reverts, tests, CI), lab dataset, scorer, split CRC, artefacts, status gating (`insufficient_n`/`stale`/`shifted`), ACI + martingale; shadow evaluation until n_min is reached | B1 preregistered and run on the first stratum to reach n_min; conformal enforcement enabled per stratum only after B1 passes |
| **P3 — Causal** (week 7–10) | RIG builder + blast radius, `request_evidence` obligations loop, shadow twin, causal-core process-DAG study (lab) | B2 run locally; obligations measurably reduce harmful outcomes (causal estimate with identification status) |
| **P4 — Harness breadth** | gnomon native `[gate]` PR, Codex, Gemini, OpenFang poller, degradation manifests | Replay fixtures for every harness; B3 in AgentLab |
| **P5 — Depth UX and research** | Web app, Phase D packet experiment, Jev and local S1 bake-off, budgets | B5 and B6 reported |
| **P6 — Deploy and transfer** | Canary with causal-core sequential reads, TrustLayer export in production, Phase E cross-domain transfer (the same envelope with a different RIG and policy pack) | Transfer is done without changing the kernel |

---

## 14. Initial ADRs

| ADR | Decision |
|---|---|
| 0001 | One Rust daemon + thin adapters; pure kernel; the lab influences the kernel only through signed, expiring artefacts. |
| 0002 | The verdict ladder is 7 rungs; combination is the most restrictive; check status is four-valued; fail class per action class. |
| 0003 | The TCB is enumerated; no LLM, network or Python on the hot path. |
| 0004 | Conformal risk control on **action-outcome labels** (not logprobs); Mondrian by action class; n_min = 100 by default; α is reported as a target. |
| 0005 | Typed-question providers (local S1, Jev) are features only (type wall); escape options are mandatory; Jev is public-only and off by default. |
| 0006 | Hashing and checkpoints are compatible with TrustLayer; events are exported rather than duplicating TrustLayer's store; SeptaCore is an evidence source. |
| 0007 | Only tighten native harness permissions; never loosen them by default. |
| 0008 | Sandbox backends declare and self-test what they confine; NemoClaw only after `onboard` works. |
| 0009 | The repo stays private until the invention disclosure is filed (S§9). |

---

## 15. Open questions for the owner

1. **TrustLayer relationship.** Should cacogate be a standalone product that *exports* to TrustLayer (the recommended default), or should it become TrustLayer's Phase 9 runtime enforcer, living inside that repository?
2. **gnomon.** Is a native `[gate]` PR to gnomon acceptable under its "no second gate" doctrine? The framing is "gnomon declares an external gate, it does not build one".
3. **Harm labels.** Which post-hoc outcomes count as *harmful* for your workflow? A proposed default list is in §3.3; this choice defines the loss.
4. **Jev access.** Do you have early-access keys, and is sending redacted action state to a cloud API acceptable for public repositories?
5. **AgentLab.** Should B3 run on Windows AgentLab (adding cacogate arms), or should a gx10 runner be stood up with the same adapters?
6. **IP.** Confirm the private-repo policy and whether an invention disclosure is in progress before any public push.
