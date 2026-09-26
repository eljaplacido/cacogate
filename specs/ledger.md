# Ledger and redaction (LD, RD)

LD: crate `cacogate-ledger`, `src/lib.rs` (hash-chained per-session JSONL).
RD: crate `cacogate-kernel`, `src/redact.rs` (`redact_args`).

- **LD-001** — When entries are appended to a session, the ledger shall verify the session's chain as intact and report the entry count. _Test:_ `crates/cacogate-ledger/src/lib.rs` `test_ld001_append_and_verify`
- **LD-002** — If an entry's body is edited on disk, then verification shall report the chain broken at that entry's index. _Test:_ `crates/cacogate-ledger/src/lib.rs` `test_ld002_any_edit_breaks_the_chain`
- **LD-003** — If an entry line is deleted on disk, then verification shall report the chain broken at the index of the deleted line. _Test:_ `crates/cacogate-ledger/src/lib.rs` `test_ld003_deleting_a_line_breaks_the_chain`
- **LD-004** — If a session's chain is broken, then the ledger shall refuse to append to it with `LedgerError::Broken`. _Test:_ `crates/cacogate-ledger/src/lib.rs` `test_ld004_refuses_to_extend_a_broken_chain`
- **LD-005** — The ledger shall keep one chain per session, list sessions by their original IDs (including IDs containing `/`), and, when reopened, continue a session's sequence numbers with the chain still verifying. _Test:_ `crates/cacogate-ledger/src/lib.rs` `test_ld005_sessions_are_separate_and_resume`
- **LD-006** — The ledger shall map distinct session IDs that differ only by `/` versus `_` to distinct file stems. _Test:_ `crates/cacogate-ledger/src/lib.rs` `test_ld006_stems_do_not_collide`
  _Notes:_ LD-006 checks one pair (`a/b` vs `a_b`), not injectivity in general.
- **RD-001** — When redacting tool arguments, the gate shall keep `command` verbatim and replace body fields such as `content` with `{redacted: true, len, digest}`. _Test:_ `crates/cacogate-kernel/src/redact.rs` `test_rd001_bodies_are_redacted_commands_kept`
  _Notes:_ RD-001 asserts `redacted` and `len` only; the `digest` field is not checked.
