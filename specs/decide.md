# Decision and verdict ladder (DE, GT)

DE: crate `cacogate-kernel`, module `src/decide.rs` (`decide`, `combine`), fixtures in shadow mode with the built-in strict policy.
GT: crate `cacogate-contracts`, module `src/lib.rs` (`Verdict`).

- **DE-001** — When the action is a benign read (`ls` via Bash, or the `Read` tool), the gate shall return `allow`. _Test:_ `crates/cacogate-kernel/src/decide.rs` `test_de001_benign_read_is_allowed`
- **DE-002** — When an invariant rule fires (`git push --force origin main`), the gate shall return `block` with a rationale that starts with the rule ID `INV-VCS-001`. _Test:_ `crates/cacogate-kernel/src/decide.rs` `test_de002_invariant_blocks`
- **DE-003** — If a mutating shell command cannot be parsed, then the gate shall return `ask_human` and include a check with status `could_not_measure`. _Test:_ `crates/cacogate-kernel/src/decide.rs` `test_de003_unparseable_mutation_asks_human`
- **DE-004** — When a file tool writes to the harness's hook configuration (`.claude/settings.json`), the gate shall return `block`. _Test:_ `crates/cacogate-kernel/src/decide.rs` `test_de004_self_protection_blocks_file_tools`
- **DE-005** — While no calibration is loaded, the gate shall report the conformal component with status `absent` and the action class as its stratum. _Test:_ `crates/cacogate-kernel/src/decide.rs` `test_de005_conformal_status_is_reported_absent`
- **DE-006** — The gate shall include a provenance check with a status other than `not_applicable` in every decision, and `combine` over no checks shall return the given default verdict. _Test:_ `crates/cacogate-kernel/src/decide.rs` `test_de006_decisions_always_carry_a_measured_check`
  _Notes:_ the name says "measured", but the body accepts any status except `not_applicable` (so `could_not_measure` satisfies it); it samples four commands only.
- **DE-007** — The gate shall never lower the combined verdict when a check is added (monotone ladder). _Test:_ `crates/cacogate-kernel/src/decide.rs` `test_de007_adding_a_check_never_loosens`
- **GT-001** — The verdict order shall equal the ladder order: each verdict's rung equals its index in `Verdict::LADDER`, so e.g. `block > ask_human` and `allow < sandbox`. _Test:_ `crates/cacogate-contracts/src/lib.rs` `test_gt001_ladder_order_matches_rungs`
- **GT-002** — Only `allow`, `sandbox` and `canary` shall be verdicts under which the action executes. _Test:_ `crates/cacogate-contracts/src/lib.rs` `test_gt002_only_first_three_rungs_execute`
