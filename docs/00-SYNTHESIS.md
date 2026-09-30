# cacogate — Synthesis

*Status: research synthesis, 2026-09-26. Inputs: the Perplexity research thread in
`../Explore the methodical landscape of these causal _.md` plus a read-only survey of
causalplatform (+ 20 worktrees), causalrust, projectcarf, RCCAEF, phdresearch, gnomon,
septacore, trustlayer, agentcenter/Oracle, routekit, skillvault, the synced Windows AgentLab
material in `~/.opencode/central-brain/runtime/inbox/`, the harness installs on gx10, and
public TypeSafe/Jev material.*

---

## 1. What the source thread actually asks for

Distilled from the thread, with the parts that carry obligations for the build:

1. **The artefact.** A *Causal–Conformal Action Gate* between an AI coding agent and every
   action it wants to take (shell, file write, VCS, deploy, MCP). Its verdicts: `sandbox`,
   `revise`, `request evidence`, `human approval`, `canary` and `block`, plus a silent
   `allow` for the common case.
2. **No pillar substitutes for another.**
   - Causality does not calibrate risk.
   - Conformal validity does not establish causal correctness or permissibility.
   - Symbolic compliance does not prove the action achieves its goal.

   The gate must keep the three as **separate, individually-reported checks**.
3. **Three planes.**
   - *Decision*: the agent loop.
   - *Assurance*: the gate.
   - *Evidence*: a tamper-evident graph linking requirement → diff → model/prompt versions →
     files read → tests → policy decisions → approvals → execution → telemetry → rollback →
     outcome.
4. **Patentable core (rated 4/5).** A runtime mechanism that:
   - intercepts a machine-executable action;
   - derives its causal execution context;
   - forms a **conformal set of permitted execution modes**;
   - checks symbolic state-transition constraints;
   - issues an **evidence-bound capability** for direct / sandboxed / canary / escalated /
     denied execution.

   Assembling labels is not inventive; the *protocol* is. The thread explicitly warns that
   public commits (CARF/CYNEPIC) may affect novelty. See §9.
5. **Sequencing.** Evidence/tracing first → symbolic rules second → causal + conformal third,
   once real outcomes exist to calibrate on.
6. **UX split.**
   - Terminal/IDE native prompt for the interactive loop: put the gate where the action
     already blocks.
   - App/dashboard/chat only for async, second-approver and audit work.
   - Never a standalone queue nobody watches.
7. **Research obligations.**
   - Phase D: decision-packet experiment. Vary explanation content; measure comprehension,
     override correctness, fatigue and response time.
   - Phase E: cross-domain transfer.
   - 12 priority RQs. §8 maps them to what cacogate can test.

## 2. What the portfolio already has, by pillar

| Pillar | What exists (best instance) | Measured? | Gap cacogate must fill |
|---|---|---|---|
| **Symbolic / policy** | TrustLayer Guardian (Rust, CSL first-match rules with typed operators and no regex; `PASS/FAIL/ESCALATE`; `ArcSwap` hot reload). causalrust `PolicyChain` + circuit breaker, rate limiter, loop detector, regorus. gnomon `bash_deny`/`bash_allow`/`write_allow`, roles as tool masks. | TrustLayer structured rules: 100% on blind corpora. Free-text/encoded PII rules 83–97% (RCCAEF ADR-041). causalrust full evaluation 5.6 µs. | No verdict *ladder* beyond 3 outcomes. **No stateful/temporal rules at runtime** (TrustLayer "Phase 9" deferred). No parsed-shell semantics; everything is regex over raw strings. |
| **Causal** | causalplatform `causal-core` (ID algorithm, refuters, E-values, Rosenbaum, anytime-valid sequences, cluster/switchback designs, typed `Abstention`) + `causal-decision` (`decide()` → `act / act_with_approval / experiment / refuse`, hash-chained ledger, follow-up calibration C0). | T0 identification 200/200 against y0. Pooled CI coverage 0.949. T6 sequential coverage 0.982–0.988. D0 all verdicts matched preregistered. C0: Clopper-Pearson follow-up reporting. | Not applied to *software actions*. No repository impact graph. `decision-thresholds-uncalibrated` is open: no real follow-ups have been measured. |
| **Conformal** | **Only one instance**: Oracle System 1 (agentcenter), split-conformal over a TF-IDF naive Bayes classifier with escape answers. phdresearch PR-0001 preregisters split CP, ACI (Gibbs–Candès) and weighted CP with SCM weights. | Oracle: coverage 0.891 [0.75, 0.975] at nominal 0.90, n=158, but the labels were Claude-made, so the result is provisional. | **No conformal risk control anywhere.** Must be written from scratch. gnomon's research explicitly *rejected* conformal gating (see §4.1). |
| **Evidence / provenance** | gnomon hash-chained JSONL audit (`AuditKind`, `canonicalJson`, signed heads). TrustLayer per-agent chains + Ed25519 checkpoints + `/v1/integrity/verify`. causalplatform `DecisionRecord` (decision ≠ execution) + HMAC **single-use verdicts bound to `payload_hash`**. CARF PROV-AGENT emitter. skillvault ATTESTATION. | Chains verify. The bug classes "verdict replay/forgery" and "caller-supplied risk score" were found and closed in causalplatform. | Nothing links a *decision* to its *realised outcome* as a calibration label. No OTel GenAI spans anywhere (gnomon has none). |
| **Autonomy / escalation** | gnomon `ApprovalGate never/on_write/always` (binary approver). Oracle G0–G7 ladder with Cynefin verdict ceilings. RCCAEF band gate: "the band decides, not the point estimate; widening uncertainty cannot buy a pass". | Oracle **SE1 supported: confidence is not a valid escalation trigger** (r = −0.26 between S1 predicted risk and wrong action). | Escalation is binary everywhere. No risk budget per session/agent. |
| **Measurement discipline** | SeptaCore DG-7 exit contract (0 pass / 1 fail / 2 could-not-measure / 3 regressed). Pre-registration files with TOML bars and `prereg_sha256`. `declared_not_implemented.json` registers. Mutation catalogues. gnomon three-point conformance (code == docs table == golden fixture). | Strong. Failing runs are kept and several false claims retracted (CARF "A+", "100% hallucination reduction"). | — (reuse wholesale) |

## 3. Harness landscape and where a gate can intercept

| Harness (gx10 version) | Interception API | Can express | Notes / degradations |
|---|---|---|---|
| **Claude Code** 2.1.283 | `PreToolUse` hook (`command` or **`http`**). Stdin `{session_id, tool_name, tool_input, tool_use_id, cwd, permission_mode, …}`. Returns `hookSpecificOutput.permissionDecision: allow/deny/ask` + `updatedInput` + `additionalContext`. Exit 2 always blocks. `PostToolUse` for outcomes. | all 7 verdicts: sandbox via `updatedInput` wrap; ask via native prompt | No global hooks today; this would be the **first action-level interceptor** in the user's setup. `defer` semantics unconfirmed. |
| **OpenCode** 1.18.32 | Plugin `tool.execute.before(input, output)`: mutate `output.args`, throw to block. `permission.ask` → `ask/deny/allow`. | all 7 | Installed `@opencode-ai/plugin` 1.14.25 is older than the binary; the ordering of `permission.ask` vs `before` needs a spike. |
| **Pi** 0.84.2 / **omp** 17.3.8 | Extension `pi.on("tool_call")` → `{block, reason, input}`; `ctx.ui.confirm()` for ask. omp revalidates the replacement input and also loads Claude-style `~/.claude/hooks/{pre,post}/<tool>.sh`. | all 7 | pi mutates input in place with no revalidation. |
| **gnomon** v0.2.3 (`~/Desktop/gnomon`) | **No hooks.** Single choke point `executeTool()` (`packages/gnomon-core/src/tools.ts:3412`). `Approver` is binary and receives no raw args. Read-only prefetch runs before approval (`prompt_loop.ts:3541`). | allow/deny only, unless gnomon is changed | Design rule: "does not build a second gate… external gates plug in". A native integration must be surface-declared, run before prefetch, map onto refusal codes 2/3/4, and add a `gate_decision` AuditKind with fixtures. |
| **Codex CLI** | `PreToolUse` (Claude-compatible JSON) behind `features.codex_hooks`. | allow/deny/ask | Bug #27833: deny is not enforced for `apply_patch`. Declare it as a degradation. |
| **Gemini CLI** | `hooks.BeforeTool` → `{decision:"deny", reason}`. | allow/deny | — |
| **OpenFang** 0.5.9 | Async only: `[approval] require_approval=[…]` + REST `/api/approvals/{id}/approve\|reject`. | allow/block/ask (async) | Daemon currently **down** (:4200 not listening). |
| **Hermes** | Windows-only (`--yolo`). | — | Out of scope on gx10; reachable through AgentLab. |
| **NemoClaw** (sandbox backend) | CLI 0.0.13: `onboard/list/<name> connect…`. **There is no `run` subcommand.** No sandboxes onboarded, no `openshell` on PATH. | — | The user's skill and ZeroClaw's `cap_sandbox_exec` call `nemoclaw run …`, which fails. **The sandbox verdict currently has no working NemoClaw backend.** |

**AgentLab (Windows, `C:\…\DIGICISU\agentlab`)** is the benchmark home. It has:
- 12 harness adapters (claude_code, opencode, opencode_docker, pi, hermes, gnomon, septacore, centralbrain_orch, …);
- a LiteLLM telemetry proxy;
- per-run worktrees;
- McNemar/Wilson stats;
- a mutation corpus;
- 1,834 recorded diffs.

Its CARF-Under-Test programme already contains **hypothesis AG4, which *is* cacogate**:

> "A typed pre-execution gate on tool calls reduces bad executions at acceptable latency".

- Measure: blocked-bad, blocked-good, added latency.
- Falsifier: false-block rate exceeds the benefit, **or added latency > 10% of turn time**.

Harness baselines from the lab: Pi pass@1 81% at 98 s; Claude Code 78% at 969 s; centralbrain_orch 65% at 124 s. SeptaCore ADR-0008 makes **merge-worthy rate** the primary metric, not pass@1.

## 4. Constraints and prior objections cacogate must answer

### 4.1 gnomon's rejection of conformal gating

`docs/HARNESS-RESEARCH-RECONCILIATION.md:216` says it:

> "needs logprobs neither endpoint kind surfaces. The '<3% hallucination' is a user-chosen α reported as a measured result."

**Answer.** Calibrate on *outcomes of actions*, not on model token probabilities.
- The nonconformity score comes from deterministic action facts, and optionally typed-question features.
- The label comes from the realised outcome recorded in the ledger (reverted, broke tests, blocked by CI, incident, benign).
- α is always reported as a *chosen target* next to the *measured* held-out coverage with a Clopper-Pearson interval.

### 4.2 Confidence is not a valid escalation trigger

Oracle SE1 found r = −0.26 between S1 predicted risk and wrong action; RT5 shows an LLM classifier with ECE 0.205.

**Consequence.** No classifier probability, and no Jev probability, may directly *be* a verdict. They may only be *features* into a scorer whose threshold is conformally certified against outcomes. This is the Oracle "type wall", applied here.

### 4.3 Calibration needs data

FINAL-STACK §9:

> "~50–100 calibration fixtures; at n=15 it would escalate nearly everything."

CRC arithmetic: with bounded loss B = 1, λ̂ admits *anything* only when n ≥ 1/α − 1. That is 19 at α = 0.05, and meaningful only at roughly 100+ per stratum.

**Consequence.**
1. Shadow mode first.
2. Mondrian strata kept coarse (action class).
3. Under-sampled strata report `could_not_measure`, which caps autonomy. It never counts as a pass.

### 4.4 Selection bias in labels (MNAR)

Blocked actions never produce outcomes, so naive calibration on executed actions is biased toward the actions the gate already let through.

**Consequence.**
- Counterfactual labels come from shadow execution in a sandboxed worktree.
- A small preregistered exploration fraction in shadow mode.
- Worst-case bounds `[s/N, (s+u)/N]` for undecidable labels (causalplatform C0).

This is RQ7, and it is a genuine research contribution.

### 4.5 Latency

- AG4 falsifier: > 10% of turn time.
- routekit: a 2.8 s process cold start dominated inference.
- SeptaCore's < 500 ms overhead target was falsified.

**Consequence.**
- A long-lived **daemon**. Claude Code's `http` hook avoids spawning a process per call.
- A deterministic hot path with p95 of tens of milliseconds.
- Causal and slow evidence work only for actions that land at `request_evidence`/`ask_human` or are irreversible.
- SeptaCore ARCHITECTURE: "activate per task class, never per keystroke".

### 4.6 Overlap with TrustLayer

TrustLayer already is a per-tool-call PASS/FAIL/ESCALATE engine with hash-chained evidence. Rebuilding a CSL clone plus a SHA-256 chain would be a duplicate.

cacogate's genuinely new parts:
1. synchronous harness adapters;
2. the 7-rung ladder with execution modes and capabilities;
3. conformal risk control on action outcomes;
4. the causal repository-impact model;
5. session-state (temporal) enforcement, which is TrustLayer's unbuilt Phase 9;
6. the decision → outcome labelling loop.

cacogate should **emit TrustLayer `AgentTraceEvent`s** (TOOL_CALL, POLICY_CHECK, HUMAN_ESCALATION, HUMAN_DECISION) and use a hash/checkpoint scheme compatible with TrustLayer's.

### 4.7 SeptaCore's named gap

`specs/adr/0004…:99-106`:

> "there is no policy evaluation point anywhere… the highest-value architectural investment available."

cacogate fills "Plane 5 — Trust/policy". SeptaCore stays the tree-level CI gate, and its 0/1/2/3 result becomes one *evidence source* for `request_evidence`.

### 4.8 Honesty machinery is the house style

Expect the user to audit every figure. The gate must:
- never report `could_not_measure` as pass or as 0;
- keep a `declared_not_implemented` register;
- tie every published number to a ledger row and a preregistration hash.

## 5. Lessons: anti-patterns found in the portfolio

| Anti-pattern | Where observed | cacogate rule |
|---|---|---|
| Heuristic bands presented as uncertainty | RCCAEF `make_band()` (`p10 = v·(1−w)`) | Uncertainty fields are either real conformal/posterior quantities with a `calibration_id`, or absent with `status: point_estimate_only`. |
| Hand-weighted risk fusion | CARF H-Neuron (0.35/0.20/0.15…) | Scorer weights are fitted on a declared split and frozen in a signed artefact. There are no literals in code. |
| Caller-supplied risk scores | causalplatform (closed bug), causalrust `RiskAwareEvaluator` | The gate computes every risk input itself from the raw action. Adapters send the action, never a score. |
| Rules no code path reaches | PR-007, PR-009, `get_llm`, provenance store | Every declared rule needs a fixture that fires it, checked in CI (dead-rule test). |
| An empty chain approves everything | causalrust (deliberate), TrustLayer default PASS | Absent/invalid policy ⇒ **strict built-in default**, not allow-all. |
| Crash instead of fail-closed | RCCAEF Guardian `float('not_a_number')` | Total parsing. Any internal error ⇒ `GateFault` ⇒ the per-class fail mode (§7 of the blueprint). |
| "Could not measure" reported as 0 or pass | RCCAEF Bayesian suite, "`bash -n` passed" (gnomon F-series) | Four-valued check status: `passed / failed / not_applicable / could_not_measure`. |
| Declaring isolation that isn't enforced | gnomon `network=false` doesn't cover bash; NemoClaw `run` missing | Each sandbox backend *declares* what it confines and self-tests it at startup. The verdict names the backend. |
| Vacuous security canaries | agentcenter FINAL-STACK §8 | Every canary has a positive control proving it can fire. |
| Headline numbers from n ≤ 8 | CARF "A+", "100% calibrated (n=8)" | Preregistered `n_min`; below it ⇒ `could_not_measure`. |
| Bounded in-memory audit | CARF `deque(maxlen=1000)` | Append-only on disk, with delete ⇒ tombstone only. |
| Silent fallbacks | CARF DoWhy → LLM simulation; 29% silent keyword fallback in GX10 A/B | Every fallback is a `degradation` record in the ledger and surfaced in the packet. |
| Importing CARF | phdresearch ADR-0003 | Reference only. Import `causal-core`/`causal-decision` by path/git pin, lab side only. |
| Guarantees for free-text policies | RCCAEF ADR-041 | Guarantees are scoped to structured rules; semantic rules are `advisory` or `defeasible`. |

## 6. Jev (TypeSafe AI) — what is known and what role it may play

**API.**
- `POST https://api.typesafe.ai/v1/systemone`, Bearer auth.
- Request body `{model, state, questions:{id:{type: noul|choice|score, instructions, criteria}}}`.
- Response carries `probabilities`, `confidence` and `usage`.
- Questions are independent and parallel.
- Pin `jev-1.13.0`; `jev-latest` is a moving alias.
- Also reachable through a LiteLLM pass-through, so the AgentLab proxy can record it (but `pricing.yaml` has no jev key, so cost logs $0).

**Vendor claims.**
- 70–500 ms; $0.042/MTok input.
- "~68% accuracy" is scored against frontier-model answers, not ground truth.
- "0% hallucination" means only no out-of-schema output.
- RLCD training is not described.
- Closed weights; waitlist; available since 2026-09-26.

**Independent audit** (jujumilk3/jev-calibration-audit, about 7k calls).
- ECE **0.023 with an "unknown" option, 0.793 without it**: on unanswerable items accuracy drops 0.95 → 0.00 at 0.79 confidence.
- Complementary probabilities sum to 0.71–1.42.
- The noul and choice forms of the same question differ by 0.125 on average.
- Robust to option order and to bundling.
- p50 280 ms, p95 397 ms.

**Lab status.** AgentLab `jev-bench.html` is a *preregistration only* (E0–E5); no measured results yet. Its decision rule:
- ECE < 0.05 ⇒ usable as a gate feature;
- 0.05–0.15 ⇒ ranking only;
- > 0.15 ⇒ decoration.

**Role in cacogate.** An optional **typed-question feature provider** on the slow path only. It answers questions such as "does this command's intent match the stated task?" or "does this diff alter authentication or authorisation logic?", always with escape options (`insufficient_evidence`, `out_of_scope`, `requires_intervention_analysis`).

Rules:
- Off by default.
- `sensitivity=public` payloads only; digests or redacted state otherwise.
- Enabled per-question only after it beats the local System 1 on the same labelled set at matched coverage.
- Never a verdict.

The local alternative is a grammar-constrained Ollama/llama-server model (qwen3.6:35b, nemotron-3-nano) exposing the same question contract. Both implement one `FeatureProvider` interface so the bake-off is a config switch.

## 7. Positioning

```
            ┌──────────── decision plane ────────────┐
 harness ──►│ Claude Code · OpenCode · Pi/omp · gnomon │── action ─┐
            │ Codex · Gemini · OpenFang               │           │
            └─────────────────────────────────────────┘           ▼
                                                   ┌── assurance plane: cacogate ─────────┐
   SeptaCore (tree/CI gate) ──evidence source────► │ facts → symbolic → causal → conformal │
   causal-core/-decision (lab + canary math) ────► │ → ladder → capability                 │
   Jev / local S1 (typed features, slow path) ───► │                                        │
                                                   └──────────────┬────────────────────────┘
                                                                  │ events (TrustLayer-compatible)
                                           ┌── evidence plane ────▼───────────────────────┐
                                           │ cacogate ledger (JSONL chain + JSONL index)  │
                                           │ → TrustLayer /v1/events · OTel GenAI spans   │
                                           │ → cb-gx10 log (milestones) → Windows brain   │
                                           └──────────────────────────────────────────────┘
   Oracle (agentcenter) = slow, read-only decision simulation; source of G-ladder, type wall, S1.
```

## 8. The 12 research questions, mapped to cacogate

| RQ | cacogate mechanism that makes it testable |
|---|---|
| 1. Identification status → conformal risk | A causal `Abstention` caps the ladder at `request_evidence`. Lab experiment: stratify calibration by identification tier and test whether coverage differs. |
| 2. Shift in agent trajectories | ACI online update + an exchangeability martingale on the ledger. Inject a new repo, model or harness and measure time-to-detect and coverage loss. |
| 3. Risk-budget allocation | Session/agent/tool budgets in `BudgetCheck`. Compare uniform vs role-stratified allocation (cf. ToolChain-CRC, role-stratified CRC). |
| 4. Do symbolic rules aid identifiability? | The repository impact graph (symbolic structure) as DAG constraints in `causal-core`. Compare identification rates with and without. |
| 5. Hard / defeasible / fuzzy / legal coexistence | Rule classes `invariant > defeasible > advisory`, with explicit precedence and an override record. |
| 6. Minimal TCB | The Rust kernel is pure, with no LLM, no network and no Python on the hot path. The TCB is enumerated in ADR-0003. |
| 7. Provenance-based incident reconstruction vs MNAR | Shadow-execution counterfactual labels + worst-case bounds. Measure the bias of executed-only calibration. |
| 8. Assurance expiry | The calibration artefact binds {harness, model, policy, scorer, toolset} digests. Any mismatch ⇒ `stale`. |
| 9. Escalation policy vs human attention | Ask-rate budget; measure harm prevented per human-minute. |
| 10. Contestability without leakage | Packets carry digests + rule ids + templated rationale; payloads are held back by sensitivity class. |
| 11. Benchmarks under perturbation | pass^k and merge-worthy rate with the gate on vs off, paired McNemar, perturbation corpora (AgentLab mutation corpus). |
| 12. Strategic adversaries | Injection corpora (gnomon containment/injection harnesses, AgentDojo/InjecAgent), calibration-gaming scenarios, and a retry-evasion detector. |

## 9. IP caution (from the source thread)

The thread rates the Action Gate protocol at 4/5 patent feasibility and flags prior public disclosures.

**Until an invention disclosure is filed with the relevant IP function:**
- keep `cacogate` in a **private** repository;
- keep a dated disclosure inventory;
- do not push the conformal-execution-mode and capability mechanism publicly.

The blueprint marks the claim-relevant components (`[IP]`).
