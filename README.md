# cacogate

Causal–conformal action gate between AI coding agents and the actions they take.
Deny-first: it can only tighten what the harness would do on its own.

Status: **P0/P1 in progress.** The symbolic gate works end to end. The
conformal and causal pillars are not built yet, so every decision reports its
conformal status as `absent`. See `honesty/declared_not_implemented.json` for
what is not built.

## Try it

```sh
cargo build --release
target/release/cacogate daemon                       # 127.0.0.1:4320, shadow mode
target/release/cacogate check -c 'git push --force origin main'
target/release/cacogate doctor                       # harnesses, hooks, sandboxes
target/release/cacogate watch                        # live decisions
```

`CACOGATE_MODE=enforce` makes adapters act on verdicts. Shadow mode only records.

## Harnesses

| Harness | Install | Verified live |
|---|---|---|
| Claude Code | plugin `claude --plugin-dir adapters/claude-code`, or `cacogate install claude-code --write` | 2026-09-26, 2.1.283 |
| OpenCode | `cp adapters/dist/opencode/cacogate.js .opencode/plugin/` | 2026-09-26, 1.18.32 |
| pi | `cp adapters/dist/pi/cacogate.js .pi/extensions/` | 2026-09-26, 0.84.2 |
| omp | `omp -e adapters/dist/pi/cacogate.js` | 2026-09-26, 17.3.8 |
| gnomon | `bash_allow = ['^cacogate run( \|$)']`, see `adapters/gnomon/` | 2026-09-26, 0.2.3 (degraded path) |

Build the TS adapters with `pnpm --dir adapters install && pnpm --dir adapters build`.

## Repository

- `docs/00-SYNTHESIS.md`: research synthesis. `docs/01-BLUEPRINT.md`: architecture and roadmap.
- `docs/CONTRACTS.md`: wire contracts, enums, API, exit codes.
- `specs/`: EARS requirements; each test is named after its requirement id.
- `contracts/v0/`: JSON Schemas. `conformance/`: goldens and shared vectors.
- `septacore check`: the repo gate (`.septacore.json`).
- `AGENTS.md`: rules for agents working on this repo.
