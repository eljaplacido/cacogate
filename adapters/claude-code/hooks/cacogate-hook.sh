#!/bin/sh
# Launch `cacogate hook claude-code`, finding the binary.
#
# Claude Code lets an action proceed on any hook exit code except 2, so a
# missing binary would fail OPEN. When the binary cannot be found this script
# fails closed (exit 2) if enforce mode is configured, and otherwise warns and
# lets the action proceed. Shadow mode, the default, only records, so in that
# mode there is nothing to enforce.
for c in "${CACOGATE_BIN:-}" "$(command -v cacogate 2>/dev/null)" \
         "$HOME/.cargo/bin/cacogate" "$HOME/.local/bin/cacogate"; do
  if [ -n "$c" ] && [ -x "$c" ]; then
    exec "$c" hook claude-code
  fi
done
cfg="${CACOGATE_CONFIG:-$HOME/.config/cacogate/config.toml}"
# Pure shell (no grep): a missing external tool must not turn into fail-open.
enforce=no
case "${CACOGATE_MODE:-}" in
  shadow) ;;
  "")
    if [ -r "$cfg" ]; then
      while IFS= read -r line || [ -n "$line" ]; do
        line=${line#"${line%%[![:space:]]*}"}   # strip leading blanks
        case "$line" in
          mode*=*\"enforce\"*|mode*=*\'enforce\'*) enforce=yes ;;
        esac
      done < "$cfg"
    fi ;;
  *) enforce=yes ;;   # enforce, or a value the binary would reject: fail closed
esac
if [ "$enforce" = yes ]; then
  echo "cacogate: binary not found (set CACOGATE_BIN or put cacogate on PATH); blocking because enforce mode is configured" >&2
  exit 2
fi
echo "cacogate: binary not found; not gating (shadow mode)" >&2
exit 0
