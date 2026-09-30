#!/usr/bin/env python3
"""Review a shadow-mode period: what the gate would have refused, and was it right.

A deny gate cannot be judged on task success (it can only lower it). The
endpoint is a 2x2 per decision: would the action have been harmful, and did
the gate hold it. This script produces both halves:

  1. A report over the decision index for a date window: decisions, would-refuse
     rate, per-rule firings, per-harness and per-class counts.
  2. A labelling queue in <data_dir>/review/labels.jsonl: every held decision,
     plus a seeded sample of allowed ones (misses can only be found there).
     Fill `label` with `harm` (the action would have done damage), `cost` (held
     as the rule intends, but you would have allowed it), `wrong` (the rule
     misread the action) or `ok` (allowed and harmless). Existing labels are
     kept when the queue is refreshed.
  3. Once labels exist, the 2x2 and the rates that follow from it. Unlabelled
     rows are counted as unlabelled, never as harmless.

Usage:
  scripts/shadow_review.py [--since 2026-09-29] [--until ...] [--sample 50]
                           [--json] [--log]    # --log: one line to cb-gx10
"""

from __future__ import annotations

import argparse
import json
import os
import random
import shutil
import subprocess
import sys
from collections import Counter
from pathlib import Path

# Same definition as the daemon's `would_hold` (engine.rs status).
EXECUTES = {"allow", "sandbox", "canary"}
LABELS = {"harm", "cost", "wrong", "ok"}


def data_dir() -> Path:
    d = os.environ.get("CACOGATE_DATA_DIR")
    return Path(d).expanduser() if d else Path.home() / ".local/share/cacogate"


def load_index(
    path: Path, since: str | None, until: str | None
) -> tuple[list[dict], int]:
    rows, bad = [], 0
    if not path.exists():
        return rows, bad
    for line in path.read_text().splitlines():
        try:
            r = json.loads(line)
        except ValueError:
            bad += 1
            continue
        day = str(r.get("recorded_at", ""))[:10]
        if since and day < since:
            continue
        if until and day > until:
            continue
        rows.append(r)
    return rows, bad


def held(r: dict) -> bool:
    return r.get("verdict") not in EXECUTES


def refresh_queue(qpath: Path, rows: list[dict], sample: int, seed: int) -> list[dict]:
    old = {}
    if qpath.exists():
        for line in qpath.read_text().splitlines():
            try:
                q = json.loads(line)
                old[q["decision_id"]] = q
            except (ValueError, KeyError):
                continue
    pick = [r for r in rows if held(r)]
    allowed = [r for r in rows if not held(r)]
    rng = random.Random(seed)
    pick += rng.sample(allowed, min(sample, len(allowed)))
    queue = []
    for r in pick:
        prev = old.get(r["decision_id"], {})
        queue.append(
            {
                "decision_id": r["decision_id"],
                "day": str(r.get("recorded_at", ""))[:10],
                "harness": r.get("harness"),
                "verdict": r.get("verdict"),
                "fired": r.get("fired", []),
                "summary": r.get("summary"),
                "label": prev.get("label"),
                "note": prev.get("note", ""),
            }
        )
    # Labels on rows outside this window are kept, not dropped.
    ids = {q["decision_id"] for q in queue}
    queue += [q for k, q in old.items() if k not in ids and q.get("label")]
    qpath.parent.mkdir(parents=True, exist_ok=True)
    qpath.write_text("".join(json.dumps(q, ensure_ascii=False) + "\n" for q in queue))
    return queue


def two_by_two(queue: list[dict]) -> dict:
    t = Counter()
    for q in queue:
        lab = q.get("label")
        h = q.get("verdict") not in EXECUTES
        if lab not in LABELS:
            t["unlabelled_held" if h else "unlabelled_allowed"] += 1
        elif lab == "harm":
            t["harm_held" if h else "harm_allowed"] += 1
        else:
            t["benign_held" if h else "benign_allowed"] += 1
        if lab in ("cost", "wrong"):
            t[f"held_{lab}"] += 1
    return dict(t)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--since")
    ap.add_argument("--until")
    ap.add_argument("--sample", type=int, default=50, help="allowed rows to queue")
    ap.add_argument("--seed", type=int, default=20260929)
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--log", action="store_true", help="send a summary via cb-gx10 log")
    a = ap.parse_args()

    dd = data_dir()
    rows, bad = load_index(dd / "index.jsonl", a.since, a.until)
    n = len(rows)
    n_held = sum(held(r) for r in rows)
    rules = Counter(f for r in rows for f in r.get("fired", []))
    held_unmeasured = sum(1 for r in rows if held(r) and not r.get("fired"))
    queue = refresh_queue(dd / "review" / "labels.jsonl", rows, a.sample, a.seed)
    rep = {
        "window": [a.since, a.until],
        "decisions": n,
        "unparseable_index_lines": bad,
        "by_mode": dict(Counter(r.get("mode") for r in rows)),
        "would_refuse": n_held,
        "would_refuse_rate": round(n_held / n, 4) if n else None,
        "held_could_not_measure": held_unmeasured,
        "by_rule": dict(rules.most_common()),
        "by_harness": dict(Counter(r.get("harness") for r in rows)),
        "by_class": dict(Counter(r.get("class") for r in rows)),
        "by_day": dict(
            sorted(Counter(str(r.get("recorded_at", ""))[:10] for r in rows).items())
        ),
        "label_queue": str(dd / "review" / "labels.jsonl"),
        "two_by_two": two_by_two(queue),
    }
    if a.json:
        print(json.dumps(rep, indent=2))
    else:
        rate = (
            "n/a (no decisions)"
            if n == 0
            else f"{n_held}/{n} = {100 * n_held / n:.2f}%"
        )
        print(
            f"decisions {n} · would refuse {rate} · of which could-not-measure {held_unmeasured}"
        )
        print(f"harness   {rep['by_harness']}")
        print(f"days      {rep['by_day']}")
        print("rules")
        for k, v in rules.most_common():
            print(f"  {k:<18} {v}")
        t = rep["two_by_two"]
        print(f"labels    queue {len(queue)} rows at {rep['label_queue']}")
        print(
            "2x2       harm: held {} / allowed {} · benign: held {} / allowed {} · "
            "unlabelled: held {} / allowed {}".format(
                t.get("harm_held", 0),
                t.get("harm_allowed", 0),
                t.get("benign_held", 0),
                t.get("benign_allowed", 0),
                t.get("unlabelled_held", 0),
                t.get("unlabelled_allowed", 0),
            )
        )
    if a.log:
        payload = {
            k: rep[k]
            for k in (
                "window",
                "decisions",
                "would_refuse",
                "would_refuse_rate",
                "held_could_not_measure",
                "by_rule",
                "by_harness",
                "two_by_two",
            )
        }
        try:
            subprocess.run(
                # cron runs with a bare PATH
                [
                    shutil.which("cb-gx10")
                    or str(Path.home() / ".opencode/central-brain/bin/cb-gx10"),
                    "log",
                    "--source",
                    "cacogate",
                    "--kind",
                    "shadow_digest",
                    "--project",
                    "cacogate",
                    "--payload",
                    json.dumps(payload),
                ],
                check=True,
                capture_output=True,
                timeout=60,
            )
        except (OSError, subprocess.SubprocessError) as e:
            print(f"cb-gx10 log failed: {e}", file=sys.stderr)
            return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
