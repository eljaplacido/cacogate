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

## Install for daily use

```sh
scripts/install-user.sh      # cargo install, systemd user unit (shadow mode), doctor
# then, yourself (an agent must not switch its own gate on):
claude plugin marketplace add "$PWD/adapters"
claude plugin install cacogate@cacogate-local
```

Stay in shadow mode for a week, read `cacogate status` per rule, then set
`mode = "enforce"` in `~/.config/cacogate/config.toml` and
`systemctl --user restart cacogate`.

Writes into other `git worktree` checkouts of the same repository, and Claude
Code's own per-project memory directory, count as inside the workspace; the
host derives both from the filesystem (RP-003, RP-004, PO-012).

## Harnesses

Install adapters at **user level** by default, outside the repositories agents
work in. Agents have been seen reading a gate's code right after a refusal; a
user-level install keeps it out of the working tree (reads of gate files are
allowed but counted in `cacogate status`).

| Harness | Install (user level) | Verified live |
|---|---|---|
| Claude Code | plugin: `claude --plugin-dir adapters/claude-code` (or a marketplace); per project: `cacogate install claude-code --write` | 2026-09-26, 2.1.283 |
| OpenCode | `cp adapters/dist/opencode/cacogate.js ~/.config/opencode/plugin/` | 2026-09-26, 1.18.32 |
| pi | `cp adapters/dist/pi/cacogate.js ~/.pi/agent/extensions/` | 2026-09-26, 0.84.2 |
| omp | `cp adapters/dist/pi/cacogate.js ~/.omp/agent/extensions/` (or `omp -e …`) | 2026-09-26, 17.3.8 |
| gnomon | `bash_allow = ['^cacogate run( \|$)']` with `cacogate` on PATH; see `adapters/gnomon/` | 2026-09-26, 0.2.3 (degraded path) |

Adapters trust only the daemon they can identify: it publishes a per-boot
nonce in `<data_dir>/daemon.json`, and a reply from anything else on the port
is ignored in favour of the local fail-closed decision. The daemon and the
adapters must share `CACOGATE_DATA_DIR`; `cacogate doctor` checks this.

## Measure before enforcing

```sh
scripts/harvest_agent_commands.py          # your past agent sessions → ~/.local/share/cacogate/corpus/
cacogate corpus ~/.local/share/cacogate/corpus/agent_commands.jsonl
cacogate status                            # live: per-rule firing, shadow would-refuse rate
```

`cacogate corpus` replays recorded tool calls through the kernel and reports
how often each rule would fire on real traffic, before anything is enforced.

Build the TS adapters with `pnpm --dir adapters install && pnpm --dir adapters build`.

## Repository

- `docs/00-SYNTHESIS.md`: research synthesis. `docs/01-BLUEPRINT.md`: architecture and roadmap.
- `docs/CONTRACTS.md`: wire contracts, enums, API, exit codes.
- `specs/`: EARS requirements; each test is named after its requirement id.
- `contracts/v0/`: JSON Schemas. `conformance/`: goldens, shared vectors and real-traffic fixtures (`conformance/corpus/`).
- `septacore check`: the repo gate (`.septacore.json`).
- `AGENTS.md`: rules for agents working on this repo.
