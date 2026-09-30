# cacogate contracts v0

The wire contracts between adapters, the daemon and the ledger. Schemas live
in `contracts/v0/*.schema.json` (JSON Schema 2020-12). Digests are
`sha256:` over RFC 8785 JCS; `conformance/vectors/digest_vectors.json` pins
Rust and TypeScript to the same bytes.

**Three-point rule (AGENTS.md article 2).** Every enum below is the same
ordered set in four places: the schema, `conformance/*.golden.json` where
one exists, this file, and the code unions. `test_cf007` checks schema
against code; `test_cf009` checks this file against the schemas. Change one
and the build fails until all four agree.

## Documents

| Document | Schema | Direction |
|---|---|---|
| `ActionEnvelope` | `action-envelope.schema.json` | adapter → daemon (`POST /v1/check`) |
| `GateDecision` | `gate-decision.schema.json` | daemon → adapter, and ledger |
| `ExecutionRecord` | `execution-record.schema.json` | adapter → daemon (`POST /v1/execution`) |
| `LedgerEntry` | `ledger-entry.schema.json` | ledger rows (hash-chained per session) |
| shared enums | `verdict.schema.json` | referenced by the above |

`action_digest = sha256(JCS({tool: tool.raw_name, args, cwd, root: repo.root}))`.
Adapters send the raw action only (article 8). The gate computes every risk
input.

## Enums

### `Verdict`

Ordered from least to most restrictive. A decision's verdict is the most
restrictive `allows` of its checks (article 3). Source of truth:
`conformance/verdicts.golden.json#verdict_ladder`.

| value | rung | executes |
|---|---|---|
| `allow` | 1 | yes |
| `sandbox` | 2 | yes, confined |
| `canary` | 3 | yes, as a canary |
| `revise` | 4 | no; the agent is told what to change |
| `request_evidence` | 5 | no; obligations listed |
| `ask_human` | 6 | no; a person decides |
| `block` | 7 | no |

Until the sandbox backends land (P1), adapters treat `sandbox` and `canary`
as `ask_human`, and never run them unconfined.

### `CheckStatus`

Four-valued (article 4). `could_not_measure` never counts as a pass for a
mutating class.

| value |
|---|
| `passed` |
| `failed` |
| `not_applicable` |
| `could_not_measure` |

### `Pillar`

| value |
|---|
| `symbolic` |
| `causal` |
| `conformal` |
| `budget` |
| `provenance` |

### `RuleClass`

Caps on what a rule of each class may impose
(`verdicts.golden.json#rule_class_max`).

| value | at most |
|---|---|
| `invariant` | `block` |
| `defeasible` | `ask_human` |
| `advisory` | `revise` |

### `ActionClass`

With the verdict each class falls back to when the gate itself faults
(`verdicts.golden.json#fail_class`, article 5).

| value | on gate fault |
|---|---|
| `read` | `allow` |
| `local_edit` | `ask_human` |
| `shell_local` | `ask_human` |
| `vcs_local` | `ask_human` |
| `vcs_remote` | `block` |
| `network` | `block` |
| `deploy` | `block` |
| `mcp` | `ask_human` |
| `delegate` | `ask_human` |

### `HarnessName`

| value | adapter |
|---|---|
| `claude_code` | `cacogate hook claude-code` (command hook) |
| `opencode` | `adapters/dist/opencode/cacogate.js` plugin |
| `pi` | `adapters/dist/pi/cacogate.js` extension |
| `omp` | same extension as pi |
| `gnomon` | `cacogate run` (degraded path, `adapters/gnomon/`) |
| `codex` | planned (P4) |
| `gemini` | planned (P4) |
| `openfang` | planned (P4) |
| `other` | `cacogate check`, `cacogate run --harness other` |

### `ToolKind`

| value |
|---|
| `shell` |
| `file_read` |
| `file_write` |
| `file_edit` |
| `search` |
| `net_fetch` |
| `mcp` |
| `delegate` |
| `other` |

### `GateMode`

| value | meaning |
|---|---|
| `shadow` | decide and record; never interfere |
| `enforce` | adapters act on the verdict |

### `ExecMode`

| value |
|---|
| `direct` |
| `sandbox` |
| `canary` |

### `Bucket`

gnomon's three-bucket outcome discipline.

| value |
|---|
| `result` |
| `refusal` |
| `apparatus_failure` |

### `ConformalStatus`

| value |
|---|
| `calibrated` |
| `insufficient_n` |
| `stale` |
| `shifted` |
| `absent` |

### `Identification`

| value |
|---|
| `identified` |
| `abstained` |
| `not_attempted` |

### `ObligationKind`

| value |
|---|
| `run` |
| `septacore_check` |
| `shadow_run` |
| `human_review` |
| `provide_context` |

### `LedgerKind`

| value |
|---|
| `envelope` |
| `decision` |
| `human` |
| `execution` |
| `outcome` |
| `degradation` |
| `checkpoint` |

## Daemon HTTP API (loopback only)

**Identity.** Every reply carries `x-cacogate-daemon: <nonce>`. After binding,
the daemon writes `<data_dir>/daemon.json` (mode 0600):
`{pid, addr, nonce, policy_digest, started}`. Adapters use a decision only if
the header equals the file's nonce, the decision's `policy_digest` equals the
file's, and the decision's `envelope_digest` is the JCS digest of the envelope
they sent. Otherwise they discard the reply, spool `daemon_identity_mismatch` or
`daemon_reply_unbound`, and decide locally (fail-closed). Adapters and daemon
must therefore share `CACOGATE_DATA_DIR`; `cacogate doctor` checks it.


| Method | Path | Body → reply |
|---|---|---|
| GET | `/v1/health` | → `{ok, version, mode, policy, policy_digest, ledger_dir, uptime_s}` |
| GET | `/v1/status` | → counters and latency, plus `by_rule` (`{fired, held}` per rule), `by_mode` (`{total, would_hold}` per mode) and `gate_reads` |
| POST | `/v1/check` | `ActionEnvelope` → `{decision: GateDecision, packet}` |
| POST | `/v1/execution` | `ExecutionRecord` → ack |
| GET | `/v1/decisions` | → recent index rows |
| GET | `/v1/decisions/{id}` | → decision, envelope and execution |
| GET | `/v1/sessions/{id}/verify` | → chain verification |

## Process exit codes

| Command | Codes |
|---|---|
| `cacogate hook claude-code` | 0 always, with a JSON decision on stdout; 2 blocks when structured output cannot be delivered or, in enforce mode, when the payload is unreadable |
| `cacogate run -- …` | the child's code (128+n on signal n); 126 refused or needs a person; 127 not startable; 2 usage. `CACOGATE_HARNESS_VERSION` and `CACOGATE_MODEL` are recorded on the envelope when set. |
| `cacogate check` | 0 would execute; 1 would not |
| `cacogate corpus <file>` | 0 report printed; 2 file or policy unreadable |
| `cacogate ledger verify` | 0 intact; 1 a chain is broken; 2 ledger unreadable |
