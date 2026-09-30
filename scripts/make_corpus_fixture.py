#!/usr/bin/env python3
"""Build the committed real-traffic fixture from a harvested corpus.

Input: the corpus from harvest_agent_commands.py (machine-local, private).
Output (to review before committing):
  conformance/corpus/real_firings.candidates.jsonl  up to --per-rule rows per rule
  conformance/corpus/real_benign.jsonl              --benign rows on which nothing fires

Rows are made portable: the session's repository becomes /work/repo, the home
directory /home/dev, agent scratch dirs /tmp/scratch. A row is kept only if,
after rewriting, the kernel fires exactly the same rules as on the original
(verified with `cacogate check --json`), so the fixture tests what really
happened. Candidates are labelled by hand into real_firings.jsonl.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import re
import subprocess
import sys

HOME = os.path.expanduser("~")


def scrub(s: str, cwd: str) -> str:
    s = re.sub(r"/tmp/claude-\d+/[^/\s'\"]+/[0-9a-f-]{36}/(scratchpad|tasks)", "/tmp/scratch", s)
    root = repo_root(cwd)
    if root:
        s = s.replace(root, "/work/repo")
    s = s.replace(HOME, "/home/dev")
    # Private network identity: tailnet/LAN addresses, host names, emails.
    s = re.sub(r"\b100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.\d{1,3}\.\d{1,3}\b", "100.64.0.1", s)
    s = re.sub(r"\b(192\.168|10\.\d{1,3})\.\d{1,3}\.\d{1,3}\b", "192.168.0.10", s)
    s = re.sub(r"(?i)\bdesktop-[a-z0-9]{5,}\b", "desktop-host", s)
    s = re.sub(r"(?i)\bgx10-[a-z0-9]{4}\b", "gx10-host", s)
    s = re.sub(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}", "user@example.com", s)
    s = re.sub(r"\beljaplacido\b", "dev", s)
    return s


def repo_root(cwd: str) -> str | None:
    d = cwd
    while d and d != "/":
        if os.path.exists(os.path.join(d, ".git")):
            return d
        d = os.path.dirname(d)
    return None


def fired(bin_: str, row: dict, home: str) -> list[str]:
    args = [bin_, "check", "--json", "--cwd", row["cwd"], "--tool", row["tool"]]
    args += ["-c", row["args"]["command"]] if "command" in row["args"] else ["--file", row["args"]["file_path"]]
    env = {"CACOGATE_CONFIG": "/nonexistent", "HOME": home, "PATH": "/usr/bin:/bin", "NO_COLOR": "1"}
    out = subprocess.run(args, capture_output=True, text=True, env=env).stdout
    try:
        d = json.loads(out)["decision"]
    except (ValueError, KeyError):
        return ["<error>"]
    return sorted(c["id"] for c in d["checks"] if c["status"] == "failed")


def portable(row: dict) -> dict:
    cwd = row["cwd"]
    args = {k: scrub(v, cwd) if isinstance(v, str) else v for k, v in row["args"].items()}
    root = repo_root(cwd)
    new_cwd = "/work/repo" + cwd[len(root):] if root and cwd.startswith(root) else scrub(cwd, cwd)
    return {"tool": row["tool"], "args": args, "cwd": new_cwd}


def main() -> int:
    ap = argparse.ArgumentParser()
    data = os.environ.get("CACOGATE_DATA_DIR", os.path.expanduser("~/.local/share/cacogate"))
    ap.add_argument("--corpus", default=os.path.join(data, "corpus", "agent_commands.jsonl"))
    ap.add_argument("--bin", default="target/release/cacogate")
    ap.add_argument("--out-dir", default="conformance/corpus")
    ap.add_argument("--per-rule", type=int, default=4)
    ap.add_argument("--benign", type=int, default=300)
    ap.add_argument("--seed", type=int, default=20260928)
    a = ap.parse_args()

    rows = [json.loads(l) for l in open(a.corpus, encoding="utf-8")]
    rnd = random.Random(a.seed)
    rnd.shuffle(rows)
    per_rule: dict[str, list[dict]] = {}
    benign: list[dict] = []
    seen: set[str] = set()
    for r in rows:
        original = fired(a.bin, r, HOME)
        if original == ["<error>"]:
            continue
        p = portable(r)
        key = json.dumps(p, sort_keys=True)
        if key in seen:
            continue
        # The rewritten row must fire exactly what the original fired.
        if fired(a.bin, p, "/home/dev") != original:
            continue
        seen.add(key)
        if not original:
            # Short, and nothing that names IP documents: the benign sample
            # is committed, and it only needs to be ordinary agent work.
            if len(key) > 400 or "patent" in key.lower():
                continue
            if len(benign) < a.benign:
                benign.append(p)
            continue
        for rule in original:
            bucket = per_rule.setdefault(rule, [])
            if len(bucket) < a.per_rule:
                bucket.append({**p, "fired": original})
                break
        if len(benign) >= a.benign and all(len(v) >= a.per_rule for v in per_rule.values()) and len(per_rule) >= 10:
            break
    os.makedirs(a.out_dir, exist_ok=True)
    with open(os.path.join(a.out_dir, "real_firings.candidates.jsonl"), "w", encoding="utf-8") as f:
        for rule in sorted(per_rule):
            for r in per_rule[rule]:
                f.write(json.dumps(r, ensure_ascii=False) + "\n")
    with open(os.path.join(a.out_dir, "real_benign.jsonl"), "w", encoding="utf-8") as f:
        for r in benign:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    print(f"{sum(map(len, per_rule.values()))} firing candidates over {len(per_rule)} rules · {len(benign)} benign", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
