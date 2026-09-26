# Shell parser (SH)

Crate `cacogate-kernel`, module `src/shell.rs` (`parse`). Pure: no I/O.

- **SH-001** — When a script contains `&&`, `;` or `||`, the parser shall split it into one command per operand, in source order. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh001_splits_on_operators`
- **SH-002** — The parser shall remove single and double quotes from words and keep a quoted string (including embedded `;` and spaces) as one argv element. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh002_quotes_are_removed_and_kept_together`
- **SH-003** — When a command has `>` or `>>` redirects to files, the parser shall record those targets as the command's writes, exclude them from argv, and not treat fd duplications such as `2>&1` as writes. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh003_redirect_targets_are_writes`
- **SH-004** — When a command is the right-hand side of a pipe, the parser shall record the argv of the command piped into it. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh004_pipe_records_source`
- **SH-005** — When a word contains a `$(…)` command substitution, the parser shall parse its body recursively and emit its commands with nesting depth 1. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh005_command_substitution_is_parsed_recursively`
- **SH-006** — When a script contains a quoted heredoc, the parser shall treat the heredoc body as data (emit no commands from it) and still report the parse as complete with the enclosing command present. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh006_heredoc_body_is_data`
- **SH-007** — When a command is `bash -c '<script>'`, the parser shall parse the script and emit its commands. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh007_bash_c_is_expanded`
- **SH-008** — If a script has an unbalanced quote, then the parser shall report status `Unparseable`. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh008_unbalanced_quote_is_unparseable`
- **SH-009** — When a script uses `eval` on a variable, the parser shall report status `Partial`. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh009_eval_is_partial`
- **SH-010** — The parser shall not treat leading `NAME=value` assignments as part of a command's argv. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh010_assignments_are_not_programs`
- **SH-011** — The parser shall treat newlines as command separators and skip `#` comment lines. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh011_newline_separated_script`
- **SH-012** — The parser shall return a result without panicking for any printable input up to 200 characters. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh012_parser_is_total`
- **SH-013** — The parser shall return a result without panicking for any input up to 120 characters drawn from shell metacharacters (quotes, `$()`, backticks, pipes, redirects, braces, `#`, `=`, newlines). _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh013_parser_total_on_shell_alphabet`
- **SH-014** — The parser shall emit the programs inside `{ …; }` groups and `if/then/fi` bodies (dropping the keywords and braces), and shall keep `find -exec … {} \;` as one command with `{}` and `;` as arguments. _Test:_ `crates/cacogate-kernel/src/shell.rs` `test_sh014_keywords_and_groups_do_not_hide_programs`
  _Notes:_ SH-012/SH-013 are property tests; they pin totality (no panic), not correctness of the result.
