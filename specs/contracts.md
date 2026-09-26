# Contracts and conformance (CF)

Crate `cacogate-contracts` (`tests/conformance.rs`, `tests/specs.rs`), plus the TS adapter JCS in `adapters/src/core/jcs.ts`.
Sources of truth: `contracts/v0/*.schema.json`, `conformance/*.golden.json`, `conformance/vectors/*`.

- **CF-001** — The Rust and TypeScript canonicalisers shall produce, for every shared digest vector, exactly the vector's canonical JSON text and `sha256:` digest. _Test:_ `crates/cacogate-contracts/tests/conformance.rs` `test_cf001_digest_vectors_match`; `adapters/test/jcs.test.ts` `CF001 digest vectors`
- **CF-002** — The verdict ladder in code shall equal `verdicts.golden.json#verdict_ladder` (order and count), and `executes` for each verdict shall match the golden table. _Test:_ `crates/cacogate-contracts/tests/conformance.rs` `test_cf002_golden_verdict_ladder_matches_code`
- **CF-003** — The fail-closed verdict for every action class in code shall equal `verdicts.golden.json#fail_class`, which shall cover exactly the classes in code. _Test:_ `crates/cacogate-contracts/tests/conformance.rs` `test_cf003_golden_fail_classes_match_code`
- **CF-004** — For every ladder vector, the most restrictive of the `allows` verdicts shall equal the expected verdict. _Test:_ `crates/cacogate-contracts/tests/conformance.rs` `test_cf004_golden_ladder_vectors`
- **CF-005** — Serialised Rust `ActionEnvelope`, sealed `GateDecision` and `ExecutionRecord` values shall validate against their v0 JSON Schemas, and a sealed decision's seal shall verify. _Test:_ `crates/cacogate-contracts/tests/conformance.rs` `test_cf005_rust_values_validate_against_schemas`
- **CF-006** — If an action envelope carries an unknown top-level field, then the schema shall reject it. _Test:_ `crates/cacogate-contracts/tests/conformance.rs` `test_cf006_schema_rejects_unknown_fields`
- **CF-007** — The schema's `Verdict` enum shall equal the code ladder in order; the `ActionClass` enum shall deserialise into code values with the same count as `ActionClass::ALL`; the `CheckStatus` enum shall deserialise with 4 members. _Test:_ `crates/cacogate-contracts/tests/conformance.rs` `test_cf007_schema_enums_match_code`
  _Notes:_ the name says "enums match code", but only `Verdict` is compared element-wise; `ActionClass` and `CheckStatus` are checked by deserialisability and count.
- **CF-008** — Every test ID in `crates/**/*.rs` and `adapters/test/*.ts` shall have a requirement in `specs/*.md`, and every requirement shall have at least one test. _Test:_ `crates/cacogate-contracts/tests/specs.rs` `test_cf008_every_test_id_has_a_requirement_and_back`
- **CF-009** — Each enum table in `docs/CONTRACTS.md` (Verdict, CheckStatus, Pillar, RuleClass, ActionClass, HarnessName, ToolKind, GateMode, ExecMode, Bucket, ConformalStatus, Identification, ObligationKind, LedgerKind) shall list exactly the corresponding v0 schema enum values, in schema order. _Test:_ `crates/cacogate-contracts/tests/docs_contracts.rs` `test_cf009_contracts_doc_tables_match_schemas`
- **CF-010** — For every action class in `verdicts.golden.json#fail_class`, the `ActionClass` table in `docs/CONTRACTS.md` shall contain the row `` | `<class>` | `<fail verdict>` | ``. _Test:_ `crates/cacogate-contracts/tests/docs_contracts.rs` `test_cf010_fail_class_table_matches_golden`
  _Notes:_ CF-010 checks golden → doc only; extra doc rows are caught by CF-009, not CF-010.
