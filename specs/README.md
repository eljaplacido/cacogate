# specs/ — requirements of record

Every behaviour of cacogate starts here as an EARS requirement (AGENTS.md, Article 1).

- **ID scheme.** `AA-NNN`: a two-letter area prefix and a three-digit number, e.g. `SH-007`.
  The test pinning it is named `test_aannn_<what>` in Rust (`test_sh007_bash_c_is_expanded`)
  or starts with `"AANNN "` in a vitest `describe` (`describe("AD001 …")`).
- **Areas.** AD/CC/RP `adapters.md` · CF `contracts.md` · DE/GT `decide.md` · EN `engine.md` ·
  FA `facts.md` · LD/RD `ledger.md` · PO `policy.md` · SH `shell.md`.
- **EARS forms.** Ubiquitous: "The gate shall …". Event-driven: "When <trigger>, the gate shall …".
  Unwanted: "If <fault>, then the gate shall …". State-driven: "While <state>, the gate shall …".
  Optional: "Where <feature>, the gate shall …".
- **Line format.** `- **AA-NNN** — <requirement>. _Test:_ <file> <test name>`, optionally
  followed by a `_Notes:_` line where the test pins less than its name suggests.
- **Scope.** A requirement states only what its test body checks. IDs are never reused.
- **Guard.** `crates/cacogate-contracts/tests/specs.rs` (CF-008) fails if a test ID has no
  requirement here, or a requirement here has no test.
