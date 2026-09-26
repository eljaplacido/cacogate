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

- If the daemon is unreachable, the hook decides locally with the same kernel,
  no looser than the fail-class table, and spools a degradation.
- If the binary is missing, `hooks/cacogate-hook.sh` exits 2 (block) when
  enforce mode is configured through `CACOGATE_MODE` or `mode = "enforce"` in
  `~/.config/cacogate/config.toml`, and otherwise warns and lets the action
  proceed.
- The kernel's self-protection covers `**/.claude/plugins/**`, so an agent
  cannot edit this plugin's hooks to switch the gate off.
