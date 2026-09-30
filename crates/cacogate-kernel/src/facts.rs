//! Deterministic facts about an action, derived only from the envelope.
//!
//! No risk input comes from the adapter: the tool kind, class, paths and
//! effects are all re-derived here (AGENTS.md article 8).

use crate::shell::{self, ParseStatus, SimpleCommand, basename};
use cacogate_contracts::{ActionClass, ActionEnvelope, ToolKind};
use serde::Serialize;
use serde_json::Value;

/// Environment the kernel needs but must not read itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KernelEnv {
    /// The user's home directory, for `~` expansion and home-relative globs.
    pub home: String,
    /// Roots the host verified as part of this workspace besides the
    /// repository root: linked worktrees of the same repository and the
    /// harness's own per-project state directory. Never taken from the
    /// adapter (article 8); the host derives them from the filesystem.
    pub linked_roots: Vec<String>,
}

/// Effect reversibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reversibility {
    /// Undo is trivial (read, local edit under VCS).
    Reversible,
    /// Undo needs a compensating action.
    Compensable,
    /// Cannot be undone by the agent.
    Irreversible,
}

/// A `git push` as understood from argv.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PushInfo {
    /// Remote name or URL, if given.
    pub remote: Option<String>,
    /// Destination branches named in refspecs (empty = current branch).
    pub targets: Vec<String>,
    /// Any force flag or `+refspec`.
    pub force: bool,
    /// `--delete` or `:ref`.
    pub delete: bool,
}

/// Facts about one simple command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandFacts {
    /// argv.
    pub argv: Vec<String>,
    /// Effective program after stripping wrappers (`sudo`, `env`, …).
    pub program: String,
    /// First non-flag argument after the program (git/cargo/npm subcommand).
    pub subcommand: Option<String>,
    /// Class of this command.
    pub class: ActionClass,
    /// Runs with elevated privilege.
    pub privileged: bool,
    /// Deletes recursively.
    pub recursive_delete: bool,
    /// Receives downloaded content on stdin and executes it.
    pub pipe_to_shell: bool,
    /// Destructive local VCS operation.
    pub destructive_vcs: bool,
    /// A recognised test / check run.
    pub is_test_run: bool,
    /// `git push` details.
    pub push: Option<PushInfo>,
}

/// Facts about the whole action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionFacts {
    /// Tool kind derived by the kernel.
    pub tool_kind: ToolKind,
    /// Overall class: the riskiest command's class.
    pub class: ActionClass,
    /// Per-command facts for shell tools.
    pub commands: Vec<CommandFacts>,
    /// Absolute (lexically normalised) paths the action may write.
    pub paths_written: Vec<String>,
    /// Absolute paths the action reads (file tools, `<` redirects).
    pub paths_read: Vec<String>,
    /// Write targets that could not be resolved statically.
    pub unresolved_writes: Vec<String>,
    /// URLs or hosts contacted.
    pub destinations: Vec<String>,
    /// Reversibility of the riskiest effect.
    pub reversibility: Reversibility,
    /// Parse status of shell input (`complete` for non-shell tools).
    pub parse_status: ParseStatus,
    /// Parser notes.
    pub parse_notes: Vec<String>,
    /// Repository root used for inside/outside decisions.
    pub root: String,
    /// Host-verified roots treated like `root` (see [`KernelEnv::linked_roots`]).
    pub linked_roots: Vec<String>,
    /// One-line summary for packets and lists.
    pub summary: String,
}

impl ActionFacts {
    /// Whether `p` is strictly inside a linked root (the root itself is not:
    /// deleting a whole worktree is still a delete outside the repository).
    pub fn strictly_in_linked_root(&self, p: &str) -> bool {
        self.linked_roots
            .iter()
            .any(|r| is_within(p, r) && p != r.trim_end_matches('/'))
    }

    /// Whether `p` is inside the repository root or a linked root.
    pub fn in_workspace(&self, p: &str) -> bool {
        is_within(p, &self.root) || self.linked_roots.iter().any(|r| is_within(p, r))
    }

    /// Whether any command is privileged.
    pub fn privileged(&self) -> bool {
        self.commands.iter().any(|c| c.privileged)
    }
    /// Whether this action is a recognised passing-test candidate.
    pub fn is_test_run(&self) -> bool {
        !self.commands.is_empty() && self.commands.iter().any(|c| c.is_test_run)
    }
    /// Whether this action edits files (file tools or shell writes inside root).
    pub fn is_edit(&self) -> bool {
        matches!(self.tool_kind, ToolKind::FileWrite | ToolKind::FileEdit)
            || !self.paths_written.is_empty()
    }
}

/// Map a harness-native tool name to a kind. Covers Claude Code, `OpenCode`,
/// pi/omp and gnomon naming.
pub fn tool_kind(raw: &str) -> ToolKind {
    if raw.starts_with("mcp__") || raw.starts_with("mcp_") {
        return ToolKind::Mcp;
    }
    match raw.to_ascii_lowercase().as_str() {
        "bash" | "shell" | "exec" | "run_shell_command" | "local_shell" | "terminal" => {
            ToolKind::Shell
        }
        "read" | "view" | "cat" | "read_file" | "notebookread" => ToolKind::FileRead,
        "write" | "write_file" | "create" => ToolKind::FileWrite,
        "edit" | "multiedit" | "notebookedit" | "patch" | "apply_patch" | "str_replace"
        | "replace" | "edit_file" => ToolKind::FileEdit,
        "glob" | "grep" | "find" | "ls" | "list" | "search" | "codesearch" | "lsp" => {
            ToolKind::Search
        }
        "webfetch" | "websearch" | "fetch" | "web_fetch" | "google_web_search" => {
            ToolKind::NetFetch
        }
        "task" | "agent" | "delegate" | "subagent" => ToolKind::Delegate,
        _ => ToolKind::Other,
    }
}

/// Extract facts from an envelope.
pub fn extract(env: &ActionEnvelope, kenv: &KernelEnv) -> ActionFacts {
    let kind = tool_kind(&env.tool.raw_name);
    let root = env
        .repo
        .as_ref()
        .and_then(|r| r.root.clone())
        .unwrap_or_else(|| env.cwd.clone());
    let root = normalize(&root, "/", &kenv.home);
    let cwd = normalize(&env.cwd, &root, &kenv.home);
    let mut facts = ActionFacts {
        tool_kind: kind,
        class: ActionClass::Read,
        commands: Vec::new(),
        paths_written: Vec::new(),
        paths_read: Vec::new(),
        unresolved_writes: Vec::new(),
        destinations: Vec::new(),
        reversibility: Reversibility::Reversible,
        parse_status: ParseStatus::Complete,
        parse_notes: Vec::new(),
        root: root.clone(),
        linked_roots: kenv
            .linked_roots
            .iter()
            .map(|r| normalize(r, "/", &kenv.home))
            .filter(|r| r.as_str() != "/" && r.as_str() != kenv.home.trim_end_matches('/'))
            .collect(),
        summary: String::new(),
    };
    let path_arg = str_arg(
        &env.args,
        &["file_path", "filePath", "path", "notebook_path", "file"],
    );
    match kind {
        ToolKind::Shell => shell_facts(env, kenv, &cwd, &mut facts),
        ToolKind::FileRead => {
            if let Some(p) = &path_arg {
                facts.paths_read.push(normalize(p, &cwd, &kenv.home));
            }
            facts.summary = format!(
                "{} {}",
                env.tool.raw_name,
                path_arg.as_deref().unwrap_or("")
            );
        }
        ToolKind::FileWrite | ToolKind::FileEdit => {
            facts.class = ActionClass::LocalEdit;
            let mut targets: Vec<String> = path_arg.iter().cloned().collect();
            targets.extend(patch_targets(&env.args));
            for p in &targets {
                facts.paths_written.push(normalize(p, &cwd, &kenv.home));
            }
            if targets.is_empty() {
                facts.unresolved_writes.push("<no path argument>".into());
            }
            facts.summary = format!("{} {}", env.tool.raw_name, targets.join(" "));
        }
        ToolKind::Search => {
            facts.summary = format!(
                "{} {}",
                env.tool.raw_name,
                str_arg(&env.args, &["pattern", "query", "path"]).unwrap_or_default()
            );
        }
        ToolKind::NetFetch => {
            facts.class = ActionClass::Network;
            if let Some(u) = str_arg(&env.args, &["url", "query"]) {
                facts.destinations.push(u.clone());
                facts.summary = format!("{} {}", env.tool.raw_name, u);
            }
        }
        ToolKind::Mcp => {
            facts.class = ActionClass::Mcp;
            facts.reversibility = Reversibility::Compensable;
            facts.summary.clone_from(&env.tool.raw_name);
        }
        ToolKind::Delegate => {
            facts.class = ActionClass::Delegate;
            facts.summary = format!(
                "{} {}",
                env.tool.raw_name,
                str_arg(&env.args, &["description", "subagent_type"]).unwrap_or_default()
            );
        }
        ToolKind::Other => {
            // Unknown tools are treated like MCP: effects unknown.
            facts.class = ActionClass::Mcp;
            facts.reversibility = Reversibility::Compensable;
            facts.summary.clone_from(&env.tool.raw_name);
        }
    }
    facts.paths_written.sort();
    facts.paths_written.dedup();
    facts.summary = truncate(facts.summary.trim(), 160);
    facts
}

fn shell_facts(env: &ActionEnvelope, kenv: &KernelEnv, cwd: &str, facts: &mut ActionFacts) {
    let command = str_arg(&env.args, &["command", "cmd", "script"]).unwrap_or_default();
    facts.summary = first_line(&command);
    let parsed = shell::parse(&command);
    facts.parse_status = parsed.status;
    facts.parse_notes = parsed.notes;
    facts.class = ActionClass::ShellLocal;
    let mut cwd = cwd.to_owned();
    let mut classes = Vec::new();
    for cmd in &parsed.commands {
        let cf = command_facts(cmd);
        // `cd` changes where later relative paths resolve.
        if cf.program == "cd"
            && cmd.depth == 0
            && let Some(dir) = cmd.argv.get(1)
        {
            cwd = normalize(dir, &cwd, &kenv.home);
        }
        for w in &cmd.writes {
            push_write(w, &cwd, kenv, facts);
        }
        for r in &cmd.reads {
            if !r.contains('…') {
                facts.paths_read.push(normalize(r, &cwd, &kenv.home));
            }
        }
        for target in write_targets(&cf, cmd) {
            push_write(&target, &cwd, kenv, facts);
        }
        for target in read_targets(&cf) {
            facts.paths_read.push(normalize(&target, &cwd, &kenv.home));
        }
        for a in &cmd.argv {
            if a.starts_with("http://") || a.starts_with("https://") {
                facts.destinations.push(a.clone());
            }
        }
        classes.push(cf.class);
        let rev = match cf.class {
            ActionClass::VcsRemote | ActionClass::Deploy => Reversibility::Irreversible,
            ActionClass::Network | ActionClass::Mcp => Reversibility::Compensable,
            _ if cf.recursive_delete || cf.destructive_vcs => Reversibility::Irreversible,
            _ => Reversibility::Reversible,
        };
        facts.reversibility = facts.reversibility.max(rev);
        facts.commands.push(cf);
    }
    facts.class = classes
        .into_iter()
        .max_by_key(|c| class_risk(*c))
        .unwrap_or(ActionClass::ShellLocal);
    if facts.commands.iter().all(|c| c.class == ActionClass::Read)
        && facts.paths_written.is_empty()
        && !facts.commands.is_empty()
    {
        facts.class = ActionClass::Read;
    }
    if !facts.paths_written.is_empty() && facts.class == ActionClass::Read {
        facts.class = ActionClass::LocalEdit;
    }
}

fn push_write(target: &str, cwd: &str, kenv: &KernelEnv, facts: &mut ActionFacts) {
    if matches!(
        target,
        "/dev/null" | "/dev/stdout" | "/dev/stderr" | "/dev/tty"
    ) {
        return;
    }
    if target.contains('$') || target.contains('…') || target.contains('*') {
        if target.starts_with("$HOME") || target.starts_with("${HOME}") {
            let rest = target
                .trim_start_matches("${HOME}")
                .trim_start_matches("$HOME");
            facts
                .paths_written
                .push(normalize(&format!("~{rest}"), cwd, &kenv.home));
            return;
        }
        facts.unresolved_writes.push(target.to_owned());
        return;
    }
    facts.paths_written.push(normalize(target, cwd, &kenv.home));
}

/// Relative risk order used to pick an action's overall class.
pub fn class_risk(c: ActionClass) -> u8 {
    match c {
        ActionClass::Read => 0,
        ActionClass::LocalEdit => 1,
        ActionClass::ShellLocal => 2,
        ActionClass::VcsLocal => 3,
        ActionClass::Delegate => 4,
        ActionClass::Mcp => 5,
        ActionClass::Network => 6,
        ActionClass::VcsRemote => 7,
        ActionClass::Deploy => 8,
    }
}

const WRAPPERS: [&str; 10] = [
    "sudo", "doas", "env", "nohup", "nice", "ionice", "timeout", "stdbuf", "command", "exec",
];

const READ_ONLY: &[&str] = &[
    "ls",
    "cat",
    "head",
    "tail",
    "less",
    "more",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "ag",
    "fd",
    "wc",
    "echo",
    "printf",
    "pwd",
    "which",
    "whereis",
    "type",
    "file",
    "stat",
    "du",
    "df",
    "tree",
    "sort",
    "uniq",
    "cut",
    "tr",
    "jq",
    "yq",
    "diff",
    "cmp",
    "date",
    "printenv",
    "uname",
    "whoami",
    "id",
    "hostname",
    "ps",
    "true",
    "false",
    "test",
    "[",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "sha256sum",
    "sha1sum",
    "md5sum",
    "cd",
    "column",
    "nl",
    "od",
    "xxd",
    "hexdump",
    "strings",
    "seq",
    "sleep",
    "wait",
    "man",
    "help",
    "history",
    "free",
    "uptime",
    "lsof",
    "nproc",
    "comm",
    "paste",
    "fold",
    "expr",
    "bc",
    "awk",
    "gawk",
    "sed",
    "find",
    "tldr",
    "pgrep",
    "ss",
    "netstat",
    "nvidia-smi",
    "lscpu",
    "lsblk",
    "locale",
    "tput",
    "clear",
    "exit",
    "set",
    "export",
    "unset",
    "source",
    ".",
    "alias",
    "read",
    "shift",
    "local",
    "return",
    "break",
    "continue",
];

const FILE_MUTATORS: &[&str] = &[
    "rm", "rmdir", "mv", "cp", "mkdir", "touch", "chmod", "chown", "chgrp", "ln", "truncate", "dd",
    "tee", "install", "patch", "unzip", "shred", "rsync",
];

const NETWORK: &[&str] = &[
    "curl", "wget", "ssh", "scp", "sftp", "nc", "ncat", "telnet", "ftp", "http", "https", "xh",
    "ping", "dig", "nslookup", "host", "aria2c",
];

const SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "fish", "python", "python3", "perl", "ruby", "node",
];

/// `systemctl` needs root only to change system units. `--user` units are the
/// caller's own, and queries (`status`, `is-active`, `list-units`, `show`, …)
/// change nothing. Measured on real agent traffic (2026-09-28, R-012): 79 of 84
/// privilege asks were `systemctl --user …` or read-only queries.
fn systemctl_privileged(args: &[&str]) -> bool {
    if args.contains(&"--user") {
        return false;
    }
    let verb = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .copied()
        .unwrap_or("");
    !matches!(
        verb,
        "" | "status"
            | "show"
            | "cat"
            | "is-active"
            | "is-enabled"
            | "is-failed"
            | "is-system-running"
            | "list-units"
            | "list-unit-files"
            | "list-timers"
            | "list-sockets"
            | "list-dependencies"
            | "list-jobs"
            | "help"
    )
}

/// Whether an interpreter will execute what arrives on stdin.
///
/// Measured on 17,398 real agent tool calls (2026-09-28, reading R-011): most pipes from
/// `curl` into an interpreter were `curl … | python3 -c "import json…"`,
/// which runs its own `-c` program and only reads the download as data.
/// This is an invariant's input, so it errs toward "executes": the download
/// is exempt only on positive evidence that the interpreter runs something
/// else (a code flag, or a script operand that looks like a file). A bare
/// interpreter, `-s`, `-`, or anything ambiguous still counts.
fn executes_stdin(program: &str, args: &[&str]) -> bool {
    let short_has =
        |a: &str, ch: char| a.starts_with('-') && !a.starts_with("--") && a.contains(ch);
    let code_flag = |a: &str| match program {
        "python" | "python3" => a == "-c" || a == "-m" || (short_has(a, 'c') && a.len() <= 4),
        "perl" | "ruby" => short_has(a, 'e') || short_has(a, 'E'),
        "node" => matches!(a, "-e" | "--eval" | "-p" | "--print"),
        // sh-family: `-c` alone or in a cluster (`-lc`, `-ec`); `-s` forces stdin.
        _ => short_has(a, 'c') && !short_has(a, 's'),
    };
    if args.iter().any(|a| code_flag(a)) {
        return false;
    }
    if args.iter().any(|a| {
        *a == "-"
            || (short_has(a, 's')
                && !matches!(program, "python" | "python3" | "perl" | "ruby" | "node"))
    }) {
        return true;
    }
    let looks_like_script = |a: &str| {
        !a.starts_with('-')
            && (a.contains('/')
                || [
                    ".py", ".sh", ".bash", ".js", ".mjs", ".cjs", ".ts", ".rb", ".pl",
                ]
                .iter()
                .any(|ext| a.ends_with(ext)))
    };
    // The first operand is the script only if nothing before it could be an
    // option value; stop at the first operand either way.
    match args.iter().find(|a| !a.starts_with('-')) {
        Some(first) => !looks_like_script(first),
        None => true,
    }
}

fn command_facts(cmd: &SimpleCommand) -> CommandFacts {
    let mut argv: &[String] = &cmd.argv;
    let mut privileged = false;
    // Strip wrappers: sudo/env/timeout/… and their own options.
    while let Some(first) = argv.first() {
        let p = basename(first);
        if !WRAPPERS.contains(&p) {
            break;
        }
        if p == "sudo" || p == "doas" {
            privileged = true;
        }
        let mut rest = argv.get(1..).unwrap_or(&[]);
        while let Some(a) = rest.first() {
            let numeric = a.chars().next().is_some_and(|c| c.is_ascii_digit());
            if a.starts_with('-') || (p == "env" && a.contains('=')) || (p == "timeout" && numeric)
            {
                rest = rest.get(1..).unwrap_or(&[]);
            } else {
                break;
            }
        }
        argv = rest;
    }
    let program = argv
        .first()
        .map(|p| basename(p).to_owned())
        .unwrap_or_default();
    let args: Vec<&str> = argv
        .get(1..)
        .unwrap_or(&[])
        .iter()
        .map(String::as_str)
        .collect();
    let subcommand = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| (*s).to_owned());
    let has = |f: &str| args.contains(&f);
    let has_short = |ch: char| {
        args.iter()
            .any(|a| a.starts_with('-') && !a.starts_with("--") && a.contains(ch))
    };
    let mut cf = CommandFacts {
        argv: cmd.argv.clone(),
        program: program.clone(),
        subcommand: subcommand.clone(),
        class: ActionClass::ShellLocal,
        privileged: privileged
            || matches!(
                program.as_str(),
                "su" | "pkexec" | "mount" | "umount" | "chroot"
            )
            || (program == "systemctl" && systemctl_privileged(&args)),
        recursive_delete: false,
        pipe_to_shell: false,
        destructive_vcs: false,
        is_test_run: false,
        push: None,
    };
    if let Some(src) = &cmd.piped_from {
        let src_prog = src.first().map_or("", |s| basename(s));
        let src_prog = if WRAPPERS.contains(&src_prog) {
            src.get(1).map_or("", |s| basename(s))
        } else {
            src_prog
        };
        if matches!(src_prog, "curl" | "wget" | "fetch" | "aria2c")
            && SHELLS.contains(&program.as_str())
            && executes_stdin(&program, &args)
        {
            cf.pipe_to_shell = true;
        }
    }
    let sub = subcommand.as_deref().unwrap_or("");
    cf.class = match program.as_str() {
        "" => ActionClass::Read,
        "git" => git_class(&args, &mut cf),
        "gh" => match sub {
            "pr" | "issue" | "release" | "repo" | "workflow" | "run" | "secret" | "variable"
            | "api" => {
                let verb = args.get(1).copied().unwrap_or("");
                // Reads, including fetching artefacts to the local disk:
                // `gh release download` was classed as a deploy on real
                // agent traffic (2026-09-28 corpus replay).
                if matches!(
                    verb,
                    "view"
                        | "list"
                        | "status"
                        | "diff"
                        | "checks"
                        | "watch"
                        | "download"
                        | "clone"
                        | "browse"
                ) {
                    ActionClass::Network
                } else if sub == "release" || sub == "repo" || sub == "secret" || sub == "workflow"
                {
                    ActionClass::Deploy
                } else {
                    ActionClass::VcsRemote
                }
            }
            _ => ActionClass::Network,
        },
        "rm" | "shred" => {
            cf.recursive_delete = has_short('r') || has_short('R') || has("--recursive");
            ActionClass::LocalEdit
        }
        "find" => {
            if has("-delete") || has("-exec") || has("-execdir") {
                ActionClass::LocalEdit
            } else {
                ActionClass::Read
            }
        }
        "sed" | "perl" => {
            if args
                .iter()
                .any(|a| a.starts_with("-i") || *a == "--in-place")
            {
                ActionClass::LocalEdit
            } else {
                ActionClass::Read
            }
        }
        "tar" => {
            if args
                .iter()
                .any(|a| a.starts_with('-') && a.contains('x') || a.starts_with('x'))
            {
                ActionClass::LocalEdit
            } else {
                ActionClass::ShellLocal
            }
        }
        "rsync" | "scp" => {
            if args.iter().any(|a| is_remote_spec(a)) {
                ActionClass::Network
            } else {
                ActionClass::LocalEdit
            }
        }
        p if FILE_MUTATORS.contains(&p) => ActionClass::LocalEdit,
        p if NETWORK.contains(&p) => ActionClass::Network,
        "npm" | "pnpm" | "yarn" | "bun" => js_pm_class(sub, &args, &mut cf),
        "npx" | "pnpx" | "bunx" => {
            let tool = sub;
            cf.is_test_run = matches!(tool, "vitest" | "jest" | "mocha" | "playwright" | "tsc");
            if matches!(tool, "vercel" | "netlify" | "wrangler" | "firebase") {
                ActionClass::Deploy
            } else {
                ActionClass::ShellLocal
            }
        }
        "cargo" => match sub {
            "test" | "nextest" => {
                cf.is_test_run = true;
                ActionClass::ShellLocal
            }
            "publish" | "yank" | "owner" | "login" => ActionClass::Deploy,
            "install" | "add" | "update" | "fetch" | "search" => ActionClass::Network,
            "clippy" | "check" | "fmt" => {
                cf.is_test_run = sub == "clippy" || sub == "check";
                ActionClass::ShellLocal
            }
            _ => ActionClass::ShellLocal,
        },
        "pytest" | "tox" | "nox" | "ctest" | "vitest" | "jest" | "mocha" => {
            cf.is_test_run = true;
            ActionClass::ShellLocal
        }
        "python" | "python3" => {
            if args.windows(2).any(|w| {
                w.first() == Some(&"-m") && matches!(w.get(1), Some(&"pytest" | &"unittest"))
            }) {
                cf.is_test_run = true;
            }
            ActionClass::ShellLocal
        }
        "uv" | "poetry" | "pip" | "pip3" | "pipx" | "conda" | "mamba" => match sub {
            "publish" | "upload" => ActionClass::Deploy,
            "install" | "add" | "sync" | "lock" | "update" | "download" => ActionClass::Network,
            "run" => {
                cf.is_test_run = args.iter().any(|a| matches!(*a, "pytest" | "tox" | "nox"));
                ActionClass::ShellLocal
            }
            _ => ActionClass::ShellLocal,
        },
        "twine" => {
            if sub == "upload" {
                ActionClass::Deploy
            } else {
                ActionClass::ShellLocal
            }
        }
        "go" => {
            cf.is_test_run = sub == "test" || sub == "vet";
            if matches!(sub, "get" | "install" | "mod") {
                ActionClass::Network
            } else {
                ActionClass::ShellLocal
            }
        }
        "make" | "just" | "task" => {
            cf.is_test_run = args
                .iter()
                .any(|a| matches!(*a, "test" | "check" | "ci" | "verify"));
            if args
                .iter()
                .any(|a| matches!(*a, "deploy" | "release" | "publish"))
            {
                ActionClass::Deploy
            } else {
                ActionClass::ShellLocal
            }
        }
        "septacore" => {
            cf.is_test_run = sub == "check";
            ActionClass::ShellLocal
        }
        "docker" | "podman" | "nerdctl" => match sub {
            "push" | "login" => ActionClass::Deploy,
            "pull" => ActionClass::Network,
            "ps" | "images" | "inspect" | "logs" | "version" | "info" => ActionClass::Read,
            _ => ActionClass::ShellLocal,
        },
        "kubectl" | "oc" => match sub {
            "get" | "describe" | "logs" | "explain" | "version" | "top" | "api-resources"
            | "config" => ActionClass::Network,
            _ => ActionClass::Deploy,
        },
        "helm" => match sub {
            "list" | "status" | "get" | "show" | "search" | "template" | "lint" | "version"
            | "repo" => ActionClass::Network,
            _ => ActionClass::Deploy,
        },
        "terraform" | "tofu" | "pulumi" => match sub {
            "apply" | "destroy" | "import" | "up" | "taint" | "untaint" | "state" => {
                ActionClass::Deploy
            }
            _ => ActionClass::Network,
        },
        "az" | "gcloud" | "aws" | "gsutil" | "doctl" | "flyctl" | "fly" | "heroku" => {
            const MUTATING: [&str; 18] = [
                "create", "delete", "deploy", "update", "set", "apply", "put", "remove", "rm",
                "start", "stop", "restart", "cp", "mv", "sync", "destroy", "scale", "upload",
            ];
            if args.iter().any(|a| MUTATING.contains(a)) {
                ActionClass::Deploy
            } else {
                ActionClass::Network
            }
        }
        "vercel" | "netlify" | "wrangler" | "firebase" | "serverless" | "sls" => {
            ActionClass::Deploy
        }
        "systemctl" | "service" | "launchctl" => {
            if matches!(sub, "status" | "list-units" | "is-active" | "show") {
                ActionClass::Read
            } else {
                ActionClass::ShellLocal
            }
        }
        p if READ_ONLY.contains(&p) => ActionClass::Read,
        _ => ActionClass::ShellLocal,
    };
    if cf.privileged && cf.class == ActionClass::Read {
        cf.class = ActionClass::ShellLocal;
    }
    if cf.pipe_to_shell {
        cf.class = ActionClass::Network;
    }
    cf
}

fn js_pm_class(sub: &str, args: &[&str], cf: &mut CommandFacts) -> ActionClass {
    match sub {
        "publish" | "unpublish" | "deprecate" | "dist-tag" | "owner" | "login" | "adduser" => {
            ActionClass::Deploy
        }
        "install" | "i" | "add" | "ci" | "update" | "upgrade" | "remove" | "uninstall" | "dlx"
        | "exec" => ActionClass::Network,
        "test" | "t" => {
            cf.is_test_run = true;
            ActionClass::ShellLocal
        }
        "run" => {
            let script = args
                .iter()
                .skip_while(|a| **a != "run")
                .nth(1)
                .copied()
                .unwrap_or("");
            cf.is_test_run = script.starts_with("test")
                || matches!(script, "check" | "typecheck" | "lint" | "verify" | "ci");
            if script.starts_with("deploy")
                || script.starts_with("release")
                || script.starts_with("publish")
            {
                ActionClass::Deploy
            } else {
                ActionClass::ShellLocal
            }
        }
        _ => ActionClass::ShellLocal,
    }
}

fn git_class(args: &[&str], cf: &mut CommandFacts) -> ActionClass {
    // Skip global options such as `-C dir`, `-c k=v`, `--git-dir=…`.
    let mut i = 0usize;
    while let Some(a) = args.get(i) {
        if *a == "-C" || *a == "-c" || *a == "--git-dir" || *a == "--work-tree" {
            i += 2;
        } else if a.starts_with('-') {
            i += 1;
        } else {
            break;
        }
    }
    let sub = args.get(i).copied().unwrap_or("");
    cf.subcommand = Some(sub.to_owned());
    let rest: Vec<&str> = args.get(i + 1..).unwrap_or(&[]).to_vec();
    let has = |f: &str| rest.contains(&f);
    let has_short = |ch: char| {
        rest.iter()
            .any(|a| a.starts_with('-') && !a.starts_with("--") && a.contains(ch))
    };
    match sub {
        "status" | "log" | "diff" | "show" | "rev-parse" | "ls-files" | "blame" | "describe"
        | "shortlog" | "grep" | "reflog" | "cat-file" | "ls-tree" | "rev-list" | "merge-base"
        | "name-rev" | "whatchanged" | "help" | "version" | "check-ignore" | "for-each-ref"
        | "show-ref" | "var" | "count-objects" => ActionClass::Read,
        "" => ActionClass::Read,
        "branch" => {
            if has("-D") || (has_short('d') && has("--force")) {
                cf.destructive_vcs = true;
                ActionClass::VcsLocal
            } else if rest.iter().all(|a| a.starts_with('-'))
                && !has("-m")
                && !has("-M")
                && !has("-d")
                && !has("-c")
            {
                ActionClass::Read
            } else {
                ActionClass::VcsLocal
            }
        }
        "tag" => {
            if rest.is_empty() || has("-l") || has("--list") {
                ActionClass::Read
            } else {
                ActionClass::VcsLocal
            }
        }
        "remote" => {
            if rest.is_empty()
                || has("-v")
                || rest.first() == Some(&"show")
                || rest.first() == Some(&"get-url")
            {
                ActionClass::Read
            } else {
                ActionClass::VcsLocal
            }
        }
        "stash" => {
            if matches!(rest.first(), Some(&"list" | &"show")) {
                ActionClass::Read
            } else {
                cf.destructive_vcs = matches!(rest.first(), Some(&"drop" | &"clear"));
                ActionClass::VcsLocal
            }
        }
        "config" => {
            if has("--get") || has("--list") || has("-l") || rest.len() <= 1 {
                ActionClass::Read
            } else {
                ActionClass::VcsLocal
            }
        }
        "worktree" => {
            if matches!(rest.first(), Some(&"list")) {
                ActionClass::Read
            } else {
                ActionClass::VcsLocal
            }
        }
        "fetch" | "pull" | "clone" | "ls-remote" | "submodule" => ActionClass::Network,
        "push" => {
            cf.push = Some(push_info(&rest));
            ActionClass::VcsRemote
        }
        "reset" => {
            cf.destructive_vcs = has("--hard") || has("--merge") || has("--keep");
            ActionClass::VcsLocal
        }
        "clean" => {
            cf.destructive_vcs = has_short('f') || has("--force");
            ActionClass::VcsLocal
        }
        "checkout" | "restore" => {
            // `git restore --staged` only unstages; it discards nothing unless
            // `--worktree` is also given (real-traffic false positive).
            let staged_only = sub == "restore"
                && (has("--staged") || has("-S"))
                && !(has("--worktree") || has("-W"));
            cf.destructive_vcs = !staged_only
                && (has("--") && rest.iter().any(|a| *a == "." || *a == ":/")
                    || (sub == "restore" && rest.contains(&"."))
                    || has("-f")
                    || has("--force"));
            ActionClass::VcsLocal
        }
        _ => ActionClass::VcsLocal,
    }
}

fn push_info(rest: &[&str]) -> PushInfo {
    let mut info = PushInfo::default();
    let mut positional = Vec::new();
    for a in rest {
        match *a {
            "-f" | "--force" | "--force-with-lease" | "--force-if-includes" | "--mirror" => {
                info.force = true;
            }
            "-d" | "--delete" => info.delete = true,
            a if a.starts_with("--force-with-lease=") => info.force = true,
            a if a.starts_with('-') => {}
            a => positional.push(a),
        }
    }
    let mut it = positional.into_iter();
    info.remote = it.next().map(str::to_owned);
    for spec in it {
        let spec = if let Some(s) = spec.strip_prefix('+') {
            info.force = true;
            s
        } else {
            spec
        };
        let dst = match spec.split_once(':') {
            Some(("", dst)) => {
                info.delete = true;
                dst
            }
            Some((_, dst)) => dst,
            None => spec,
        };
        let dst = dst.trim_start_matches("refs/heads/");
        if !dst.is_empty() && dst != "HEAD" {
            info.targets.push(dst.to_owned());
        }
    }
    info
}

fn is_remote_spec(a: &str) -> bool {
    !a.starts_with('-')
        && !a.starts_with('/')
        && !a.starts_with('.')
        && a.split_once(':')
            .is_some_and(|(h, _)| !h.is_empty() && !h.contains('/'))
}

/// Paths a file-mutating command writes, from its argv.
fn write_targets(cf: &CommandFacts, cmd: &SimpleCommand) -> Vec<String> {
    let args: Vec<&String> = operand_args(cmd, &cf.program);
    match cf.program.as_str() {
        "rm" | "rmdir" | "mkdir" | "touch" | "chmod" | "chown" | "chgrp" | "truncate" | "shred"
        | "tee" => {
            let skip = usize::from(matches!(cf.program.as_str(), "chmod" | "chown" | "chgrp"));
            args.iter().skip(skip).map(|s| (*s).clone()).collect()
        }
        "mv" | "cp" | "ln" | "install" | "rsync" => {
            let mut v: Vec<String> = args.last().map(|s| (*s).clone()).into_iter().collect();
            if cf.program == "mv" {
                // the source disappears too
                v.extend(args.iter().rev().skip(1).map(|s| (*s).clone()));
            }
            v.retain(|p| !is_remote_spec(p));
            v
        }
        "sed" | "perl" if cf.class == ActionClass::LocalEdit => {
            args.iter().skip(1).map(|s| (*s).clone()).collect()
        }
        "dd" => cmd
            .argv
            .iter()
            .filter_map(|a| a.strip_prefix("of=").map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn read_targets(cf: &CommandFacts) -> Vec<String> {
    match cf.program.as_str() {
        "cat" | "head" | "tail" | "less" | "more" | "source" | "." => cf
            .argv
            .iter()
            .skip(1)
            .filter(|a| !a.starts_with('-') && !a.contains('…') && !a.contains('$'))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

fn operand_args<'a>(cmd: &'a SimpleCommand, program: &str) -> Vec<&'a String> {
    let start = cmd
        .argv
        .iter()
        .position(|a| basename(a) == program)
        .map_or(1, |p| p + 1);
    let mut out = Vec::new();
    let mut after_dashdash = false;
    for a in cmd.argv.iter().skip(start) {
        if !after_dashdash && a == "--" {
            after_dashdash = true;
            continue;
        }
        if !after_dashdash && a.starts_with('-') {
            continue;
        }
        out.push(a);
    }
    out
}

fn patch_targets(args: &serde_json::Map<String, Value>) -> Vec<String> {
    // apply_patch / patch tools embed paths in a patch body.
    let body = str_arg(args, &["patch", "patchText", "input", "diff"]).unwrap_or_default();
    let mut out = Vec::new();
    for line in body.lines() {
        for prefix in [
            "*** Update File: ",
            "*** Add File: ",
            "*** Delete File: ",
            "+++ b/",
            "--- a/",
        ] {
            if let Some(p) = line.strip_prefix(prefix) {
                let p = p.trim();
                if !p.is_empty() && p != "/dev/null" && !out.iter().any(|x| x == p) {
                    out.push(p.to_owned());
                }
            }
        }
    }
    out
}

/// First string value among candidate keys.
pub fn str_arg(args: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| args.get(*k).and_then(Value::as_str).map(str::to_owned))
}

/// Lexically normalise `p` against `base`, expanding `~`. Never touches disk.
pub fn normalize(p: &str, base: &str, home: &str) -> String {
    let expanded = if p == "~" {
        home.to_owned()
    } else if let Some(rest) = p.strip_prefix("~/") {
        format!("{}/{}", home.trim_end_matches('/'), rest)
    } else {
        p.to_owned()
    };
    let joined = if expanded.starts_with('/') {
        expanded
    } else {
        format!("{}/{}", base.trim_end_matches('/'), expanded)
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Whether `path` is `root` or inside it.
pub fn is_within(path: &str, root: &str) -> bool {
    let root = root.trim_end_matches('/');
    root.is_empty() || path == root || path.starts_with(&format!("{root}/"))
}

fn first_line(s: &str) -> String {
    let line = s.lines().next().unwrap_or("");
    if s.lines().nth(1).is_some() {
        format!("{line} …")
    } else {
        line.to_owned()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cacogate_contracts::{CONTRACT_VERSION, Harness, HarnessName, Repo, Tool};
    use serde_json::json;

    #[allow(clippy::needless_pass_by_value)]
    pub(crate) fn env_for(tool: &str, args: Value) -> ActionEnvelope {
        let args = args.as_object().cloned().unwrap_or_default();
        ActionEnvelope {
            contract_version: CONTRACT_VERSION.into(),
            envelope_id: "e".into(),
            harness: Harness {
                name: HarnessName::ClaudeCode,
                version: "t".into(),
                adapter_version: "t".into(),
                interactive: Some(true),
            },
            session_id: "s".into(),
            turn_id: None,
            tool_use_id: "t1".into(),
            agent: None,
            model: None,
            tool: Tool {
                raw_name: tool.into(),
                kind: None,
            },
            action_digest: cacogate_contracts::action_digest(tool, &args, "/repo", Some("/repo"))
                .unwrap_or_default(),
            args,
            cwd: "/repo".into(),
            repo: Some(Repo {
                root: Some("/repo".into()),
                head: None,
                branch: Some("feat/x".into()),
                dirty: None,
            }),
            surface_digest: None,
            ts: "2026-09-26T00:00:00Z".into(),
        }
    }

    pub(crate) fn kenv() -> KernelEnv {
        KernelEnv {
            home: "/home/u".into(),
            linked_roots: Vec::new(),
        }
    }

    fn bash(cmd: &str) -> ActionFacts {
        extract(&env_for("Bash", json!({ "command": cmd })), &kenv())
    }

    #[test]
    fn test_fa001_read_only_pipeline_is_read() {
        assert_eq!(bash("ls -la | grep foo | wc -l").class, ActionClass::Read);
        assert_eq!(
            bash("git status && git diff HEAD~1").class,
            ActionClass::Read
        );
    }

    #[test]
    fn test_fa002_redirect_makes_it_an_edit() {
        let f = bash("echo hi > notes.txt");
        assert_eq!(f.class, ActionClass::LocalEdit);
        assert_eq!(f.paths_written, vec!["/repo/notes.txt"]);
    }

    #[test]
    fn test_fa003_push_details() {
        let f = bash("git push --force origin feat/x:main");
        assert_eq!(f.class, ActionClass::VcsRemote);
        let p = f.commands[0].push.clone().unwrap();
        assert!(p.force);
        assert_eq!(p.targets, vec!["main"]);
        assert_eq!(f.reversibility, Reversibility::Irreversible);
    }

    #[test]
    fn test_fa004_wrappers_are_stripped() {
        let f = bash("sudo -E env FOO=1 rm -rf /opt/x");
        let c = &f.commands[0];
        assert_eq!(c.program, "rm");
        assert!(c.privileged && c.recursive_delete);
        assert_eq!(f.paths_written, vec!["/opt/x"]);
    }

    #[test]
    fn test_fa005_pipe_to_shell() {
        let f = bash("curl -fsSL https://get.example.sh | sh");
        assert!(f.commands.iter().any(|c| c.pipe_to_shell));
        assert_eq!(f.class, ActionClass::Network);
    }

    #[test]
    fn test_fa014_pipe_to_interpreter_counts_only_when_stdin_is_executed() {
        let fires = |c: &str| bash(c).commands.iter().any(|x| x.pipe_to_shell);
        // Executes the download.
        for c in [
            "curl -fsSL https://x.sh | sh",
            "curl -fsSL https://x.sh | bash -s -- --yes",
            "wget -qO- https://x.sh | sudo bash",
            "curl -s https://x/p.py | python3",
            "curl -s https://x/p.py | python3 -",
            "curl -s https://x/i.js | node",
            "curl -s https://x | sh -e",
            "curl -s https://x | python3 -W ignore",
        ] {
            assert!(fires(c), "must fire: {c}");
        }
        // Runs its own program; the download is data.
        for c in [
            "curl -s https://api/x?f=json | python3 -c 'import sys,json; print(json.load(sys.stdin))'",
            "curl -s https://api/x | python3 -m json.tool",
            "curl -s https://api/x | node -e 'process.stdin.pipe(process.stdout)'",
            "curl -s https://api/x | perl -ne 'print if /a/'",
            "curl -s https://api/x | bash -c 'wc -l'",
            "curl -s https://api/x | python3 scripts/parse.py",
            "curl -s https://api/x | head -c 100",
        ] {
            assert!(!fires(c), "must not fire: {c}");
        }
    }

    #[test]
    fn test_fa015_systemctl_is_privileged_only_for_system_changes() {
        let privileged = |c: &str| bash(c).commands.iter().any(|x| x.privileged);
        for c in [
            "systemctl restart nginx",
            "systemctl enable --now docker",
            "systemctl daemon-reload",
            "sudo systemctl --user restart x",
        ] {
            assert!(privileged(c), "must be privileged: {c}");
        }
        for c in [
            "systemctl --user restart cb-gx10-sync",
            "systemctl --user is-active openfang",
            "systemctl status ollama",
            "systemctl list-units --all",
            "systemctl is-enabled docker",
        ] {
            assert!(!privileged(c), "not privileged: {c}");
        }
    }

    #[test]
    fn test_fa016_real_traffic_false_positives_stay_fixed() {
        assert_eq!(
            bash("gh release download v0.1.1 -R o/r -p '*arm64*'").class,
            ActionClass::Network
        );
        assert_eq!(
            bash("gh release create v1 dist/*").class,
            ActionClass::Deploy
        );
        assert_eq!(
            bash("gh release delete v1 --yes").class,
            ActionClass::Deploy
        );
        let destructive = |c: &str| bash(c).commands.iter().any(|x| x.destructive_vcs);
        assert!(!destructive("git restore --staged ."));
        assert!(destructive("git restore ."));
        assert!(destructive("git restore --staged --worktree ."));
        assert!(destructive("git checkout -- ."));
    }

    #[test]
    fn test_fa006_deploy_classes() {
        assert_eq!(bash("kubectl apply -f k8s/").class, ActionClass::Deploy);
        assert_eq!(bash("npm publish").class, ActionClass::Deploy);
        assert_eq!(bash("kubectl get pods").class, ActionClass::Network);
        assert_eq!(bash("az group delete -n x").class, ActionClass::Deploy);
    }

    #[test]
    fn test_fa007_tests_are_recognised() {
        assert!(bash("cargo test -p api").is_test_run());
        assert!(bash("cd web && pnpm run test:unit").is_test_run());
        assert!(bash("uv run pytest -q").is_test_run());
        assert!(!bash("cargo build").is_test_run());
    }

    #[test]
    fn test_fa008_cd_changes_resolution() {
        let f = bash("cd sub && touch a.txt");
        assert_eq!(f.paths_written, vec!["/repo/sub/a.txt"]);
    }

    #[test]
    fn test_fa009_file_tools() {
        let f = extract(
            &env_for(
                "Edit",
                json!({"file_path": "/repo/src/x.rs", "old_string": "a", "new_string": "b"}),
            ),
            &kenv(),
        );
        assert_eq!(f.class, ActionClass::LocalEdit);
        assert_eq!(f.paths_written, vec!["/repo/src/x.rs"]);
        let f = extract(
            &env_for("write", json!({"filePath": "~/.bashrc", "content": "x"})),
            &kenv(),
        );
        assert_eq!(f.paths_written, vec!["/home/u/.bashrc"]);
    }

    #[test]
    fn test_fa010_normalize() {
        assert_eq!(normalize("../x/./y", "/repo/a", "/h"), "/repo/x/y");
        assert_eq!(
            normalize("~/.ssh/id_rsa", "/repo", "/home/u"),
            "/home/u/.ssh/id_rsa"
        );
        assert!(is_within("/repo/a", "/repo"));
        assert!(!is_within("/repository", "/repo"));
    }

    #[test]
    fn test_fa011_mcp_and_unknown_tools() {
        assert_eq!(
            extract(&env_for("mcp__github__create_pr", json!({})), &kenv()).class,
            ActionClass::Mcp
        );
        assert_eq!(
            extract(&env_for("SomethingNew", json!({})), &kenv()).class,
            ActionClass::Mcp
        );
    }

    #[test]
    fn test_fa012_apply_patch_paths() {
        let body = "*** Begin Patch\n*** Update File: src/a.rs\n@@\n-x\n+y\n*** Add File: src/b.rs\n+z\n*** End Patch";
        let f = extract(&env_for("apply_patch", json!({"input": body})), &kenv());
        assert_eq!(f.paths_written, vec!["/repo/src/a.rs", "/repo/src/b.rs"]);
    }

    #[test]
    fn test_fa013_heredoc_commit_is_vcs_local() {
        let f = bash("git commit -m \"$(cat <<'EOF'\nfeat: x\n\nrm -rf /\nEOF\n)\"");
        assert_eq!(f.class, ActionClass::VcsLocal);
        assert!(f.commands.iter().all(|c| c.program != "rm"));
    }
}
