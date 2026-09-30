# AGENTS.md — cacogate constitution

cacogate is a causal–conformal action gate: a deny-first daemon that coding-agent harnesses consult
before executing a tool action. Read `docs/00-SYNTHESIS.md` (why) and `docs/01-BLUEPRINT.md` (what/how)
before changing anything.

## Articles

1. **Spec first.** Every behaviour starts as an EARS requirement in `specs/` with an ID; its test is
   named `test_<id>_…`. Code without a requirement is not merged.
2. **Three-point rule.** Any enum or contract change updates, in the same PR: `contracts/v0/*.schema.json`,
   `conformance/*.golden.json`, `docs/CONTRACTS.md`, and the code unions (Rust/Python/TS). CI enforces it.
3. **Monotone ladder.** A check may only lower autonomy. Never add a code path that raises the verdict
   above the most restrictive `allows`.
4. **Four-valued honesty.** `could_not_measure` is never reported as pass, 0, or allow for a mutating class.
   Every fallback writes a `degradation`.
5. **Fail closed per class.** Internal faults map through `conformance/verdicts.golden.json#fail_class`.
   No panics in non-test code (`unwrap/expect/indexing/panic` are denied lints).
6. **Pure kernel.** `crates/cacogate-kernel` performs no I/O, reads no clock, spawns nothing.
7. **Type wall.** Probabilities from typed-question providers (local S1, Jev) are scorer *features* only.
   Nothing in the ladder accepts a probability. No LLM text in decision packets.
8. **No caller-supplied risk.** Adapters send raw actions; the gate computes every risk input.
9. **Numbers need a ledger row.** Any figure in docs/README cites `honesty/readings.jsonl` or a benchmark
   result carrying `prereg_sha256`. α is a *target*; coverage is *measured*.
10. **Tighten, never loosen.** By default cacogate cannot grant what the harness's native permissions deny.
11. **Self-protection.** Agents must not edit gate/hook configuration or keys; the policy enforces it on
    this repo too.
12. **Private until disclosed.** Do not push to a public remote (ADR-0009).

## Gate

`septacore check` (declared in `.septacore.json`): 0 pass · 1 fail · 2 could-not-measure · 3 regressed.
Before claiming "done": run it, then consult `honesty/declared_not_implemented.json`.

## Toolchains

Rust 1.94 (edition 2024) · Python 3.12 via uv · TypeScript 7 + pnpm 9 + biome · JSON Schema 2020-12 + JCS.

## gx10 norms

Never create a file named `brain.db`; the ledger lives under `~/.local/share/cacogate/` (JSONL chains and `index.jsonl`).
Durable milestones go through `cb-gx10 log`. Prefer local Ollama for any model call in the lab.
