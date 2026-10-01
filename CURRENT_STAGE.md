# Current stage

_As of 2026-10-01. Figures cite `honesty/readings.jsonl` (article 9); what is
not built is listed in `honesty/declared_not_implemented.json`._

## In one paragraph

cacogate is a deny-first gate that coding-agent harnesses consult before each
tool call. The **symbolic gate (P0/P1) is built and in daily use on gx10 in
shadow mode**: a loopback daemon decides every Claude Code tool call against
the built-in strict policy and records the decision in a hash-chained ledger,
but refuses nothing yet. The **conformal and causal pillars are not built**;
every decision reports its conformal status as `absent`. The current milestone
is a shadow week (from 2026-09-30) to measure what enforcement would cost on
real traffic before turning it on.

## What works

| Area | State | Evidence |
|---|---|---|
| Kernel: shell parser, action facts, policy, verdict ladder, decision packets | built, pure (no I/O) | `specs/` FA, SH, PO, DE requirements; `septacore check` GREEN |
| Daemon on `127.0.0.1:4320`, per-boot identity nonce, `status` / `watch` / `explain` / `doctor` | built, installed as systemd user unit | `specs/engine.md`, `crates/cacogate-cli/tests/identity.rs` |
| Ledger: hash-chained per-session JSONL, redaction, decision index | built (`ledger verify`) | `specs/ledger.md` |
| Claude Code plugin (user level, local marketplace `adapters/.claude-plugin`) | built, enabled on gx10 | CC-001..007, AD-016 |
| OpenCode, pi, omp adapters | built; user-level install via `scripts/install-user.sh --harnesses` | AD-001, AD-017, AD-018 |
| gnomon | degraded path only (`cacogate run --` wrapper) | AD-010..015 |
| Real-traffic replay (`cacogate corpus`) and labelled fixtures | built | PO-009..011 |
| Linked roots: other worktrees of the same repo, Claude Code's own memory dir | built | PO-012, RP-003, RP-004 |
| Shadow review: per-rule report, harm × held labelling queue, daily digest to Central Brain | built (`scripts/shadow_review.py`, cron 21:55) | — |
| agentcenter Oracle ingests the decision index | built (agentcenter `oracle/connectors/cacogate.py`) | agentcenter `test_cacogate_*` |

## What the numbers say so far

Replaying recorded Claude Code tool calls from gx10 through the kernel (no
session history, `builtin:strict`):

| Step | Would-refuse rate | Reading |
|---|---|---|
| First replay | 8.13% of 17,398 calls | R-010 |
| After parser and fact fixes (pipe-to-interpreter, systemctl, `gh download`, …) | 3.83% | R-014 |
| After linked roots (worktrees, Claude memory dir) | 2.10% of 17,499 calls | R-016, R-017 |

What still holds on real traffic is mostly what a person should see: edits to
`~/.ssh/config`, Central Brain device config, systemd units, cross-repository
writes, `sudo`, `git reset --hard`, and pushes. 132 holds are
could-not-measure (variables the parser cannot resolve), not rule firings
(R-013).

The external report behind the conformal design (R-001..R-007) found that
split conformal holds its nominal coverage on exchangeable data (0.904 at
0.90) and **does not** on a new experiment (0.78 at 0.90, 0.58 at 0.80), while
α still moves the escalation rate monotonically (0–33%). Conformal will
therefore ship as an escalation dial, never as a coverage promise.

## The shadow week (2026-09-30 → ~2026-10-07)

- The daemon records; nothing is refused. `cacogate status` shows the
  per-rule would-refuse rate.
- **Plugin hooks load only when a Claude Code session starts.** Sessions that
  were already running at install time are not gated; restart them.
- Daily at 21:55, `scripts/shadow_review.py --since 2026-09-30 --log` sends a
  digest to Central Brain via `cb-gx10 log`.
- At the end of the week: label the queue in
  `~/.local/share/cacogate/review/labels.jsonl` (`harm`, `cost`, `wrong`,
  `ok`). The endpoint is the harm × held 2×2, not task success: a deny gate can
  only lower a pass rate.
- Then decide per rule what to enforce (`mode = "enforce"` in
  `~/.config/cacogate/config.toml`).

## Not built (from the honesty register)

Conformal apply and calibration (CRC, ACI, shift martingale) · Repository
Impact Graph (causal pillar) · sandbox backends (sandbox/canary verdicts ask a
person today) · single-use capabilities · temporal monitor · ledger
checkpoints and exporters (TrustLayer, OTel, PROV) · unix socket · codex,
gemini, openfang adapters · native gnomon integration · TUI/web UI · Python
lab · preregistered benchmarks B0–B8 · agent skills.

Waiting on the owner: review of the Claude-assigned corpus labels
(`conformance/corpus/`), the shadow-week labels, and the open questions in
`docs/01-BLUEPRINT.md` §15.

## Next

1. Finish the shadow week, label, and turn on enforce for the rules whose
   held decisions are mostly `harm`.
2. Build the conformal tier from shadow-mode near-misses (not from a list of
   commands that sound dangerous), as an escalation dial with per-harness
   coverage reported.
3. Sandbox backends, so model-written code (e.g. agentcenter
   `harness/repair_loop.py`) can be routed rather than only recorded.
4. A standing randomised holdout for the B-series benchmarks.

## Try it

```sh
scripts/install-user.sh                   # binary, daemon (shadow), doctor
claude plugin marketplace add "$PWD/adapters" && claude plugin install cacogate@cacogate-local
cacogate status                           # live per-rule would-refuse rate
scripts/shadow_review.py --since 2026-09-30
```
