# Real-traffic fixtures

PO-002 proves every rule *can* fire, on commands written by hand. These
files hold commands that coding agents actually issued, so the tests can show
what the rules *do*: whether they fire on real hazards (PO-009), stay silent
on ordinary work (PO-010), and keep fixed false positives fixed (PO-011).

| File | Rows | What it is |
|---|---|---|
| `real_firings.jsonl` | 30 | Real commands on which a rule fired, labelled `hazard` (what the rule exists to stop) or `policy` (fires as designed on routine work; the rule's cost). Each has a `why`. |
| `real_benign.jsonl` | 180 | A seeded random sample of real commands on which nothing fires. |
| `real_fixed_false_positives.jsonl` | 7 | Real commands that fired wrongly before 2026-09-28 (`curl … \| python3 -c`, `systemctl --user`, `gh release download`). |

## Provenance

- Source: Claude Code transcripts on gx10, 17,398 unique tool calls across
  17 projects, harvested 2026-09-28 by `scripts/harvest_agent_commands.py`.
  The full corpus stays in the machine's data dir and is **not committed**.
- `scripts/make_corpus_fixture.py` makes rows portable: the session's repo
  becomes `/work/repo`, the home dir `/home/dev`, agent scratch dirs
  `/tmp/scratch`, and private network identities (tailnet/LAN addresses,
  host names, emails, the user name) are replaced. A row is kept only if the
  kernel fires exactly the same rules on the rewritten row as on the original.
- Rows that look like they carry a credential are dropped at harvest. Long
  commands (heredoc bodies, release notes) and anything naming IP documents
  were left out, so the fixture carries ordinary agent work only.
- Labels were assigned by Claude (Opus 5.5) on 2026-09-28 and have **not been
  reviewed by the owner**. `policy` rows are where the owner decides whether
  the rule's cost is acceptable (e.g. sibling-worktree writes).

## Regenerate

```sh
scripts/harvest_agent_commands.py                  # → ~/.local/share/cacogate/corpus/
cargo build --release
cacogate corpus ~/.local/share/cacogate/corpus/agent_commands.jsonl   # the report
scripts/make_corpus_fixture.py                     # → real_firings.candidates.jsonl + real_benign.jsonl
```

Then label the candidates by hand into `real_firings.jsonl`. Numbers from a
replay go into `honesty/readings.jsonl` (R-010…R-015).
