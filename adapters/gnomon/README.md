# cacogate for gnomon (degraded path)

gnomon has no hook system, and by design it builds no second gate of its own
(gnomon `docs/DESIGN.md`). Until the native `[gate]` integration exists
(blueprint §5), cacogate plugs in the way SeptaCore does: as a shell command
that the role's allow-list forces every command through.

## Install

In `.gnomon/roles.toml`, for each role that holds `bash`:

```toml
bash_allow = ['^cacogate run( |$)']   # or the absolute path to the binary
```

Keep the role's existing `bash_deny`. Deny wins in gnomon, so the two layers
stack and cacogate can only tighten.

Append to `.gnomon/system.md`:

```markdown
## Command gate
Every shell command must be run through the gate: prefix it with `cacogate run -- `.
Example: `cacogate run -- ls -la`. A refused command exits 126 and prints why;
do not retry or work around a refusal.
```

Both files are part of gnomon's surface hash, so the change is visible to
anything that pins it. `cacogate doctor` reports gnomon as gated when
`roles.toml` mentions `cacogate run`.

## Why this holds

- gnomon matches `bash_allow` **per pipeline segment** and refuses command
  substitution and redirection outright. `cacogate run -- curl … | sh` is
  refused by gnomon because the `sh` segment is not on the list.
- `cacogate run` executes argv directly, with no shell, so the command the gate
  judged is the command that runs. `cacogate run -- sh -c '…'` is judged on
  the `-c` body: the kernel parses nested shell bodies.
- If the daemon is down, the wrapper decides locally with the same kernel and
  never goes looser than the fail-class table (exit 126 on refusal).

## Exit codes

| code | meaning |
|---|---|
| child's code | the gate allowed it and it ran (128+n if killed by signal n) |
| 126 | refused, or it needs a person (`cacogate run` has no one to ask) |
| 127 | could not be started |
| 2 | usage error |

## Verified

2026-09-26, gnomon 0.2.3+aafea2a, role `implementor`, `gnomon task --yes`,
local Qwen3.6-35B via llama.cpp, daemon in enforce mode. `ls` ran;
`git push --force origin main` was blocked by INV-VCS-001 (exit 126);
`curl … | sh` was refused by gnomon's own segment check. Ledger rows were
labelled `gnomon`.

## Degradations

See `degradations.json`. The native integration removes all of them.
