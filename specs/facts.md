# Action facts (FA)

Crate `cacogate-kernel`, module `src/facts.rs` (`extract`, `normalize`, `is_within`). Pure: no I/O.
Fixtures use cwd and repo root `/repo`, home `/home/u`.

- **FA-001** — When every command in a shell action is read-only (e.g. `ls | grep | wc`, `git status && git diff`), the gate shall classify the action as `read`. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa001_read_only_pipeline_is_read`
- **FA-002** — When a shell command redirects output to a file, the gate shall classify the action as `local_edit` and record the target, resolved against cwd, in `paths_written`. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa002_redirect_makes_it_an_edit`
- **FA-003** — When a shell action is `git push --force <remote> <src>:<dst>`, the gate shall classify it as `vcs_remote`, mark the push as forced, record `<dst>` as the target branch, and mark the action irreversible. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa003_push_details`
- **FA-004** — When a command is wrapped in `sudo` and/or `env` (with flags and assignments), the gate shall strip the wrappers to find the real program, mark the command privileged, detect a recursive delete, and record the deleted path as written. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa004_wrappers_are_stripped`
- **FA-005** — When a download is piped into a shell (`curl … | sh`), the gate shall flag the command as `pipe_to_shell` and classify the action as `network`. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa005_pipe_to_shell`
- **FA-006** — The gate shall classify `kubectl apply`, `npm publish` and `az group delete` as `deploy`, and read-only cluster queries such as `kubectl get` as `network`. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa006_deploy_classes`
- **FA-007** — The gate shall recognise `cargo test`, `pnpm run test:*` (including after `cd … &&`) and `uv run pytest` as test runs, and shall not recognise `cargo build` as one. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa007_tests_are_recognised`
- **FA-008** — When a script runs `cd <dir> &&` before a write, the gate shall resolve the written path relative to `<dir>`. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa008_cd_changes_resolution`
- **FA-009** — When a file tool (`Edit`, `write`, case-insensitive; `file_path` or `filePath`) is used, the gate shall classify it as `local_edit` and record the path, expanding `~` to the home directory. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa009_file_tools`
- **FA-010** — The gate shall normalise paths by resolving `.`/`..` against cwd and `~` against home, and shall test containment on path components (so `/repository` is not within `/repo`). _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa010_normalize`
- **FA-011** — When the tool is an `mcp__*` tool or an unrecognised tool name, the gate shall classify the action as `mcp`. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa011_mcp_and_unknown_tools`
- **FA-012** — When the tool is `apply_patch`, the gate shall record every `Update File` and `Add File` path in the patch body, resolved against cwd, as written. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa012_apply_patch_paths`
- **FA-013** — When a `git commit` message is supplied through a heredoc substitution, the gate shall classify the action as `vcs_local` and emit no command from the message text. _Test:_ `crates/cacogate-kernel/src/facts.rs` `test_fa013_heredoc_commit_is_vcs_local`
