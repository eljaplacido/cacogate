# cacogate plugin for Claude Code

Registers `PreToolUse`, `PostToolUse` and `PostToolUseFailure` hooks that call
`cacogate hook claude-code`, plus `/cacogate:status` and `/cacogate:explain`.

## Install

```sh
cargo install --path crates/cacogate-cli     # or set CACOGATE_BIN
cacogate daemon &                            # shadow mode by default
claude --plugin-dir adapters/claude-code     # one session, or add via a marketplace
```

The per-project alternative (no plugin) is `cacogate install claude-code --write`,
which writes the same hooks into `.claude/settings.local.json`.

## Failure behaviour

- The hook trusts only the daemon it can identify (`daemon.json` nonce, policy
  digest, reply bound to the action). If the daemon is unreachable or cannot be
  identified, the hook decides locally with the same kernel,
  no looser than the fail-class table, and spools a degradation.
- If the binary is missing, `hooks/cacogate-hook.sh` exits 2 (block) when
  enforce mode is configured through `CACOGATE_MODE` or `mode = "enforce"` in
  `~/.config/cacogate/config.toml`, and otherwise warns and lets the action
  proceed.
- The kernel's self-protection covers `**/.claude/plugins/**`, so an agent
  cannot edit this plugin's hooks to switch the gate off.

## What gets recorded

Each envelope carries Claude Code's version (from `AI_AGENT`) and the model
(read from the session transcript). Known gap: on the **first tool call of a
session** the transcript has no assistant message yet, so that one envelope has
no model; every later call does (verified live, 2026-09-28).
