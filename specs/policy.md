# Policy (PO)

Crate `cacogate-kernel`, module `src/policy.rs` (`Policy::builtin_strict`, `compile`, `evaluate`, `validate`).
Fixtures evaluate on branch `feat/x` unless stated.

- **PO-001** — The built-in strict policy shall pass `validate`. _Test:_ `crates/cacogate-kernel/src/policy.rs` `test_po001_builtin_policy_is_valid`
- **PO-002** — Every rule in the built-in strict policy shall have a fixture command on which it fires (no dead rules). _Test:_ `crates/cacogate-kernel/src/policy.rs` `test_po002_every_builtin_rule_has_a_firing_fixture`
- **PO-003** — When a session has no edits, the built-in policy shall fire no rule for benign commands (`ls -la`, `cargo test`, `git status`, `git commit`, redirect into the repo, `rm -rf target`, `git push origin feat/x`, `cat .env.example`). _Test:_ `crates/cacogate-kernel/src/policy.rs` `test_po003_benign_commands_fire_nothing`
- **PO-004** — While the session has an edit with no passing test run after it, the policy shall fire `DEF-VCS-003` (and only it) on `git push`; the rule shall not fire before any edit or after a recorded test pass. _Test:_ `crates/cacogate-kernel/src/policy.rs` `test_po004_push_needs_fresh_tests_only_after_edits`
- **PO-005** — When a forced push names no target and the current branch is `main`, the policy shall fire exactly `INV-VCS-001`. _Test:_ `crates/cacogate-kernel/src/policy.rs` `test_po005_unknown_push_target_on_protected_branch`
- **PO-006** — If an advisory or defeasible rule declares `allows = block`, then `validate` shall reject the policy. _Test:_ `crates/cacogate-kernel/src/policy.rs` `test_po006_class_cap_is_enforced`
- **PO-007** — The policy shall evaluate commands nested in `$(…)` substitutions and `bash -c` bodies (`INV-VCS-001`, `INV-FS-001` fire on them). _Test:_ `crates/cacogate-kernel/src/policy.rs` `test_po007_nested_substitution_is_gated`
  _Notes:_ PO-006 mutates only the first advisory/defeasible rule found.
- **PO-008** — If a command writes to or deletes a path where a harness adapter is installed (Claude Code plugins under `.claude/plugins/`, omp extensions under `.omp/extensions/` or `.omp/agent/extensions/`), then the policy shall fire the self-protection invariant `INV-SELF-001`; a similarly named path outside those directories shall not fire it. _Test:_ `crates/cacogate-kernel/src/policy.rs` `test_po008_adapter_install_locations_are_self_protected`
