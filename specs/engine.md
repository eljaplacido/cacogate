# Daemon engine (EN)

Crate `cacogate-daemon`, module `src/engine.rs` (`Engine::check`, `Engine::execution`, `Engine::status`).
Fixtures run the built-in strict policy in shadow mode over a temporary ledger.

- **EN-001** — While a session has an executed edit and no passing test run since, the engine shall return `request_evidence` (packet contains `REQUEST EVIDENCE`) for `git push`; a failing test run shall not count as evidence, and after a passing test run the push shall be `allow`. _Test:_ `crates/cacogate-daemon/src/engine.rs` `test_en001_edit_then_push_requests_evidence_until_tests_pass`
- **EN-002** — When the engine restarts over an existing ledger, it shall rebuild session state by replay (a prior executed write still makes `git push` `request_evidence`) and the session chain shall verify. _Test:_ `crates/cacogate-daemon/src/engine.rs` `test_en002_state_survives_restart_via_ledger_replay`
- **EN-003** — If an envelope's args do not match its `action_digest`, then the engine shall reject it with `DigestMismatch`. _Test:_ `crates/cacogate-daemon/src/engine.rs` `test_en003_tampered_digest_is_rejected`
- **EN-004** — When the engine records a checked action, the ledger file shall not contain the plaintext of body arguments (e.g. file content). _Test:_ `crates/cacogate-daemon/src/engine.rs` `test_en004_ledger_stores_redacted_bodies`
  _Notes:_ the body only asserts absence of the secret string; it does not check the redaction record's shape.
- **EN-005** — The engine shall seal every decision (seal verifies) and count it in status by total, would-hold and verdict. _Test:_ `crates/cacogate-daemon/src/engine.rs` `test_en005_decisions_are_sealed_and_indexed`
- **EN-006** — The engine shall compute latency percentiles by nearest rank, returning 0 for an empty sample. _Test:_ `crates/cacogate-daemon/src/engine.rs` `test_en006_percentile_nearest_rank`
