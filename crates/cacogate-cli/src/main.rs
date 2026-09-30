//! `cacogate` — causal–conformal action gate CLI.

mod corpus;
mod gate;
mod hook;
mod http;
mod run;
mod ui;

use cacogate_contracts::{
    ActionEnvelope, CONTRACT_VERSION, GateMode, Harness, HarnessName, Tool, action_digest,
};
use cacogate_daemon::{Config, Engine};
use cacogate_harness::{claude, repo};
use cacogate_kernel::{DecideInput, SessionState, decide, extract, packet};
use cacogate_ledger::Ledger;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{Map, Value, json};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "cacogate",
    version,
    about = "Causal–conformal action gate for AI coding agents"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the daemon (loopback HTTP API).
    Daemon {
        /// Listen address (default from config, 127.0.0.1:4320).
        #[arg(long)]
        addr: Option<String>,
        /// Override mode.
        #[arg(long, value_enum)]
        mode: Option<ModeArg>,
    },
    /// Harness hook entry points (reads the hook payload on stdin).
    Hook {
        /// Harness.
        #[arg(value_enum)]
        harness: HookHarness,
    },
    /// Gate a command, then run it directly (for harnesses without hooks).
    ///
    /// Exit: the command's own code; 126 refused; 127 not startable.
    Run {
        /// Harness on whose behalf the command runs.
        #[arg(long, value_enum, default_value = "gnomon")]
        harness: RunHarness,
        /// The command and its arguments (after `--`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        argv: Vec<String>,
    },
    /// Evaluate an action locally without the daemon (no session history).
    Check {
        /// Tool name as the harness names it (Bash, Edit, Write, Read, …).
        #[arg(long, default_value = "Bash")]
        tool: String,
        /// Shell command (for shell tools).
        #[arg(long, short = 'c')]
        command: Option<String>,
        /// File path (for file tools).
        #[arg(long)]
        file: Option<String>,
        /// Working directory (default: current).
        #[arg(long)]
        cwd: Option<String>,
        /// Print the full decision JSON.
        #[arg(long)]
        json: bool,
    },
    /// Replay recorded agent tool calls (JSON lines) through the kernel and
    /// report per-rule firing and the would-refuse rate on real traffic.
    Corpus {
        /// JSON-lines file (`scripts/harvest_agent_commands.py` writes one).
        path: String,
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
        /// Examples to show per rule.
        #[arg(long, default_value_t = 3)]
        examples: usize,
        /// Write every row on which a rule fired to this file, for labelling.
        #[arg(long)]
        fired_out: Option<String>,
    },
    /// Stream decisions as they happen.
    Watch {
        /// Print the latest N and exit.
        #[arg(long)]
        once: bool,
        /// How many recent decisions to show first.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show a decision's checks, rationale and what happened next.
    Explain {
        /// Decision id (d_…).
        id: String,
    },
    /// Counters: verdicts, classes, harnesses, latency.
    Status,
    /// Ledger operations.
    Ledger {
        #[command(subcommand)]
        cmd: LedgerCmd,
    },
    /// Check the installation: daemon, data dir, harnesses, hooks, sandboxes.
    Doctor,
    /// Print or install harness hook configuration.
    Install {
        /// Harness.
        #[arg(value_enum)]
        harness: HookHarness,
        /// Merge into `<dir>/.claude/settings.local.json` instead of printing.
        #[arg(long)]
        write: bool,
        /// Project directory (default: current).
        #[arg(long)]
        dir: Option<String>,
    },
}

#[derive(Subcommand)]
enum LedgerCmd {
    /// Verify every session chain (exit 1 if any is broken).
    Verify {
        /// Only this session.
        #[arg(long)]
        session: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Shadow,
    Enforce,
}

#[derive(Clone, Copy, ValueEnum)]
enum RunHarness {
    Gnomon,
    Codex,
    Gemini,
    Other,
}

impl From<RunHarness> for HarnessName {
    fn from(h: RunHarness) -> HarnessName {
        match h {
            RunHarness::Gnomon => HarnessName::Gnomon,
            RunHarness::Codex => HarnessName::Codex,
            RunHarness::Gemini => HarnessName::Gemini,
            RunHarness::Other => HarnessName::Other,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum HookHarness {
    ClaudeCode,
}

fn main() -> ExitCode {
    ui::init_local_offset();
    let cli = Cli::parse();
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            // A broken config must not silently disable a hook: the hook
            // path treats it as enforce-with-defaults and stays fail-closed.
            eprintln!("cacogate: config error: {e}");
            if matches!(cli.cmd, Cmd::Hook { .. }) {
                return ExitCode::from(2);
            }
            if matches!(cli.cmd, Cmd::Run { .. }) {
                return ExitCode::from(126);
            }
            return ExitCode::from(1);
        }
    };
    let code = match cli.cmd {
        Cmd::Daemon { addr, mode } => daemon(cfg, addr, mode),
        Cmd::Hook {
            harness: HookHarness::ClaudeCode,
        } => hook::run(&cfg),
        Cmd::Run { harness, argv } => run::run(&cfg, harness.into(), &argv),
        Cmd::Check {
            tool,
            command,
            file,
            cwd,
            json,
        } => check(&cfg, &tool, command, file, cwd, json),
        Cmd::Corpus {
            path,
            json,
            examples,
            fired_out,
        } => corpus::run(&cfg, &path, json, examples, fired_out.as_deref()),
        Cmd::Watch { once, limit } => ui::watch(&cfg, once, limit),
        Cmd::Explain { id } => ui::explain(&cfg, &id),
        Cmd::Status => ui::status(&cfg),
        Cmd::Ledger {
            cmd: LedgerCmd::Verify { session },
        } => ledger_verify(&cfg, session.as_deref()),
        Cmd::Doctor => ui::doctor(&cfg),
        Cmd::Install {
            harness: HookHarness::ClaudeCode,
            write,
            dir,
        } => install_claude(write, dir),
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn daemon(mut cfg: Config, addr: Option<String>, mode: Option<ModeArg>) -> i32 {
    if let Some(a) = addr {
        cfg.addr = a;
    }
    if let Some(m) = mode {
        cfg.mode = match m {
            ModeArg::Shadow => GateMode::Shadow,
            ModeArg::Enforce => GateMode::Enforce,
        };
    }
    let ledger = match Ledger::open(&cfg.data_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "cacogate: cannot open ledger at {}: {e}",
                cfg.data_dir.display()
            );
            return 1;
        }
    };
    let engine = match Engine::new(ledger, &cfg.policy(), cfg.home.clone(), cfg.mode) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("cacogate: built-in policy failed to compile: {e}");
            return 1;
        }
    };
    eprintln!(
        "cacogate daemon {} · {} · mode {:?} · policy {} · ledger {}",
        env!("CARGO_PKG_VERSION"),
        cfg.addr,
        cfg.mode,
        engine.policy().policy.name,
        cfg.data_dir.display()
    );
    let nonce = match cacogate_daemon::identity::fresh_nonce() {
        Ok(n) => n,
        Err(e) => {
            eprintln!("cacogate: cannot draw an identity nonce: {e}");
            return 1;
        }
    };
    let state = Arc::new(cacogate_daemon::server::AppState {
        engine,
        started: Instant::now(),
        nonce,
    });
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("cacogate: runtime: {e}");
            return 1;
        }
    };
    match rt.block_on(cacogate_daemon::server::serve(
        &cfg.addr,
        state,
        &cfg.data_dir,
    )) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("cacogate: serve {}: {e}", cfg.addr);
            1
        }
    }
}

fn check(
    cfg: &Config,
    tool: &str,
    command: Option<String>,
    file: Option<String>,
    cwd: Option<String>,
    as_json: bool,
) -> i32 {
    let cwd = cwd.unwrap_or_else(|| {
        std::env::current_dir().map_or_else(|_| "/".into(), |p| p.to_string_lossy().into_owned())
    });
    let mut args = Map::new();
    if let Some(c) = command {
        args.insert("command".into(), json!(c));
    }
    if let Some(f) = file {
        args.insert("file_path".into(), json!(f));
    }
    let repo = repo::probe(&cwd);
    let root = repo.as_ref().and_then(|r| r.root.clone());
    let Ok(digest) = action_digest(tool, &args, &cwd, root.as_deref()) else {
        return 1;
    };
    let envelope = ActionEnvelope {
        contract_version: CONTRACT_VERSION.into(),
        envelope_id: "cli".into(),
        harness: Harness {
            name: HarnessName::Other,
            version: "cli".into(),
            adapter_version: env!("CARGO_PKG_VERSION").into(),
            interactive: Some(true),
        },
        session_id: "cli".into(),
        turn_id: None,
        tool_use_id: "cli".into(),
        agent: None,
        model: None,
        tool: Tool {
            raw_name: tool.into(),
            kind: None,
        },
        args,
        cwd,
        repo,
        surface_digest: None,
        ts: cacogate_daemon::engine::now_rfc3339(),
        action_digest: digest,
    };
    let Ok(policy) = cfg.policy().compile() else {
        eprintln!("cacogate: policy failed to compile");
        return 1;
    };
    let started = Instant::now();
    let facts = extract(
        &envelope,
        &cacogate_daemon::engine::kernel_env(&cfg.home, &envelope),
    );
    let mut d = decide(&DecideInput {
        envelope: &envelope,
        facts: &facts,
        session: &SessionState::default(),
        policy: &policy,
        mode: GateMode::Enforce,
        decision_id: "check".into(),
        envelope_digest: String::new(),
    });
    d.latency_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "decision": d, "facts": facts }))
                .unwrap_or_default()
        );
    } else {
        println!(
            "{}",
            ui::paint_packet(&packet::render(&d, &facts), d.verdict)
        );
    }
    i32::from(!d.verdict.executes())
}

fn ledger_verify(cfg: &Config, only: Option<&str>) -> i32 {
    let ledger = match Ledger::open(&cfg.data_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cacogate: {e}");
            return 2;
        }
    };
    let sessions = match only {
        Some(s) => vec![s.to_owned()],
        None => ledger.sessions().unwrap_or_default(),
    };
    let mut broken = 0;
    let mut entries = 0;
    for s in &sessions {
        let r = ledger.verify_session(s);
        entries += r.entries;
        if !r.ok() {
            broken += 1;
            println!(
                "BROKEN  {s}  at seq {}: {}",
                r.broken_at.unwrap_or(0),
                r.reason.unwrap_or_default()
            );
        }
    }
    println!(
        "{} sessions, {entries} entries, {broken} broken",
        sessions.len()
    );
    i32::from(broken > 0)
}

#[allow(clippy::indexing_slicing)] // serde_json::Value indexing does not panic
fn install_claude(write: bool, dir: Option<String>) -> i32 {
    let bin = std::env::current_exe()
        .map_or_else(|_| "cacogate".into(), |p| p.to_string_lossy().into_owned());
    let cfg = claude::hooks_config(&bin);
    if !write {
        println!("{}", serde_json::to_string_pretty(&cfg).unwrap_or_default());
        eprintln!("# add this to .claude/settings.local.json (or rerun with --write)");
        return 0;
    }
    let dir = dir.unwrap_or_else(|| ".".into());
    let path = std::path::Path::new(&dir).join(".claude/settings.local.json");
    let mut current: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    if current.to_string().contains("cacogate") && current.to_string().contains("hook claude-code")
    {
        println!("already installed in {}", path.display());
        return 0;
    }
    let Some(obj) = current.as_object_mut() else {
        eprintln!("cacogate: {} is not a JSON object", path.display());
        return 1;
    };
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let Some(hooks) = hooks.as_object_mut() else {
        return 1;
    };
    for (event, groups) in cfg["hooks"].as_object().into_iter().flatten() {
        let list = hooks.entry(event.clone()).or_insert_with(|| json!([]));
        if let (Some(list), Some(add)) = (list.as_array_mut(), groups.as_array()) {
            list.extend(add.iter().cloned());
        }
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(
        &path,
        serde_json::to_string_pretty(&current).unwrap_or_default() + "\n",
    ) {
        Ok(()) => {
            println!("installed Claude Code hooks in {}", path.display());
            0
        }
        Err(e) => {
            eprintln!("cacogate: write {}: {e}", path.display());
            1
        }
    }
}
