#!/bin/sh
# Install cacogate for this user: binary, daemon service, and the Claude Code
# plugin marketplace. Idempotent. Hook activation is left to a person: an
# agent must not switch its own gate on or off (AGENTS.md article 11).
set -eu
repo=$(cd "$(dirname "$0")/.." && pwd)

# `--harnesses`: copy the built OpenCode and pi/omp adapters into their
# user-level extension dirs. Run it yourself: it switches gates on.
if [ "${1:-}" = "--harnesses" ]; then
  pnpm --dir "$repo/adapters" build
  for d in "$HOME/.config/opencode/plugin"; do
    mkdir -p "$d" && cp "$repo/adapters/dist/opencode/cacogate.js" "$d/cacogate.js" && echo "installed $d/cacogate.js"
  done
  for d in "$HOME/.pi/agent/extensions" "$HOME/.omp/agent/extensions"; do
    mkdir -p "$d" && cp "$repo/adapters/dist/pi/cacogate.js" "$d/cacogate.js" && echo "installed $d/cacogate.js"
  done
  "$HOME/.cargo/bin/cacogate" doctor | sed -n '/^harnesses/,/^sandbox/p'
  exit 0
fi

cargo install --locked --path "$repo/crates/cacogate-cli" --root "$HOME/.cargo" -j2

unit_dir="$HOME/.config/systemd/user"
mkdir -p "$unit_dir"
cp "$repo/adapters/systemd/cacogate.service" "$unit_dir/cacogate.service"
systemctl --user daemon-reload
systemctl --user enable --now cacogate.service
systemctl --user restart cacogate.service

"$HOME/.cargo/bin/cacogate" doctor || true

cat <<MSG

Daemon installed and running (shadow mode unless config.toml says enforce).
To gate every Claude Code session, run these yourself:

  claude plugin marketplace add $repo/adapters
  claude plugin install cacogate@cacogate-local

Then start a new Claude Code session and check: cacogate status
OpenCode, pi and omp (user level): $0 --harnesses
MSG
