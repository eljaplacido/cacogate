#!/usr/bin/env python3
"""Harvest the tool calls real agents issued into a replay corpus.

The dead-rule test (PO-002) proves each rule *can* fire, on fixtures written
by hand. Whether a rule *will* fire on what agents actually do, and how often
it fires on harmless commands, needs real traffic. This script collects it
from local Claude Code transcripts (~/.claude/projects/**.jsonl).

Output: JSON lines {tool, args, cwd, source, harness_version, model}, deduped,
with file bodies dropped (paths only) and any row that looks like it carries a
credential removed entirely. The output is written OUTSIDE the repository by
default (cacogate's data dir): it contains commands from every project on this
machine. Only the reviewed fixture in conformance/corpus/ is committed.

Usage:
  scripts/harvest_agent_commands.py [--out PATH] [--root ~/.claude/projects]
  cacogate corpus PATH            # replay through the kernel, per-rule report
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import sys

# A row matching any of these is dropped, not redacted: a partial secret in a
# corpus is still a secret.
SECRET = re.compile(
    r"sk-ant-|sk-[A-Za-z0-9]{20,}|ghp_[A-Za-z0-9]{20,}|github_pat_|gho_[A-Za-z0-9]{20,}"
    r"|AKIA[0-9A-Z]{16}|xox[baprs]-|-----BEGIN [A-Z ]*PRIVATE KEY"
    r"|(?i:authorization:\s*\S)|(?i:bearer\s+[A-Za-z0-9._-]{16,})"
    r"|(?i:(pass(word|wd)?|secret|token|api[_-]?key)\s*[=:]\s*['\"]?[^\s'\"$]{8,})"
)
# Opaque high-entropy tokens: 40+ chars with no path separators, mixing
# letters and digits. Pure hex of digest length (git SHAs, sha256) is not a
# secret and is common in agent commands.
TOKEN = re.compile(r"(?<![A-Za-z0-9/._-])[A-Za-z0-9+=_-]{40,}(?![A-Za-z0-9/._-])")
HEX_DIGEST = re.compile(r"^(?:[0-9a-f]{40}|[0-9a-f]{64})$")


def looks_secret(s: str) -> bool:
    if SECRET.search(s):
        return True
    for m in TOKEN.finditer(s):
        tok = m.group(0)
        if HEX_DIGEST.match(tok):
            continue
        if re.search(r"[0-9]", tok) and re.search(r"[A-Za-z]", tok):
            return True
    return False

FILE_TOOLS = {"Read": "file_path", "Write": "file_path", "Edit": "file_path", "NotebookEdit": "notebook_path"}
MAX_COMMAND = 2000


def rows_from_claude(root: str):
    files = glob.glob(os.path.join(root, "*", "*.jsonl")) + glob.glob(
        os.path.join(root, "*", "*", "subagents", "*.jsonl")
    )
    for path in sorted(files):
        project = os.path.relpath(path, root).split(os.sep)[0]
        try:
            fh = open(path, encoding="utf-8", errors="replace")
        except OSError:
            continue
        with fh:
            for line in fh:
                if '"tool_use"' not in line:
                    continue
                try:
                    m = json.loads(line)
                except ValueError:
                    continue
                msg = m.get("message") or {}
                content = msg.get("content")
                if not isinstance(content, list):
                    continue
                for b in content:
                    if not isinstance(b, dict) or b.get("type") != "tool_use":
                        continue
                    name, inp = b.get("name"), b.get("input") or {}
                    if name == "Bash":
                        cmd = inp.get("command")
                        if not isinstance(cmd, str) or not cmd.strip() or len(cmd) > MAX_COMMAND:
                            continue
                        args = {"command": cmd}
                    elif name in FILE_TOOLS:
                        p = inp.get(FILE_TOOLS[name])
                        if not isinstance(p, str):
                            continue
                        args = {"file_path": p}  # bodies never leave the transcript
                    else:
                        continue
                    yield {
                        "tool": name,
                        "args": args,
                        "cwd": m.get("cwd") or "/",
                        "source": f"claude_code:{project}",
                        "harness_version": m.get("version") or "unknown",
                        "model": msg.get("model") or None,
                    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    data = os.environ.get("CACOGATE_DATA_DIR", os.path.expanduser("~/.local/share/cacogate"))
    ap.add_argument("--out", default=os.path.join(data, "corpus", "agent_commands.jsonl"))
    ap.add_argument("--root", default=os.path.expanduser("~/.claude/projects"))
    a = ap.parse_args()

    seen: set[tuple[str, str, str]] = set()
    kept = dropped_secret = dup = 0
    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    tmp = a.out + ".tmp"
    with open(tmp, "w", encoding="utf-8") as out:
        os.chmod(tmp, 0o600)
        for r in rows_from_claude(a.root):
            blob = json.dumps(r["args"], sort_keys=True)
            if looks_secret(blob):
                dropped_secret += 1
                continue
            key = (r["tool"], blob, r["cwd"])
            if key in seen:
                dup += 1
                continue
            seen.add(key)
            out.write(json.dumps(r, ensure_ascii=False) + "\n")
            kept += 1
    os.replace(tmp, a.out)
    print(
        f"kept {kept} · dropped {dropped_secret} that looked like credentials · {dup} duplicates → {a.out}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
