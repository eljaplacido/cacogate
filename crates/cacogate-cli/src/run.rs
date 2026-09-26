//! `cacogate run -- <argv…>`: gate a command, then execute it directly.
//!
//! Exit codes: the child's own code when it ran (128+signal when killed);
//! 126 when the gate refused it or it needs a person (the wrapper has no one
//! to ask); 127 when it could not be started; 2 on a usage error.

use crate::gate;
use crate::http::Client;
use cacogate_contracts::{
    Bucket, CONTRACT_VERSION, ExecMode, ExecutionRecord, GateMode, HarnessName,
};
use cacogate_daemon::Config;
use cacogate_daemon::engine::now_rfc3339;
use cacogate_harness::{Enforcement, enforcement, wrap};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Exit code for a refused command (POSIX "found but not executable").
pub(crate) const REFUSED: i32 = 126;

/// Gate and run `argv` on behalf of `harness`.
pub(crate) fn run(cfg: &Config, harness: HarnessName, argv: &[String]) -> i32 {
    let Some(program) = argv.first() else {
        eprintln!("cacogate run: nothing to run (usage: cacogate run -- <command> [args…])");
        return 2;
    };
    let cwd =
        std::env::current_dir().map_or_else(|_| "/".into(), |p| p.to_string_lossy().into_owned());
    let session = session_id(harness);
    let tool_use_id = format!(
        "run-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    );
    let envelope = match wrap::envelope(harness, &session, &tool_use_id, argv, &cwd, &now_rfc3339())
    {
        Ok(e) => e,
        Err(e) => {
            gate::spool(cfg, "payload_unreadable", &e.to_string());
            if cfg.mode == GateMode::Enforce {
                eprintln!("cacogate: cannot identify this command ({e}); refusing");
                return REFUSED;
            }
            return exec(cfg, program, argv, &session, &tool_use_id, None);
        }
    };
    let Some(g) = gate::decide(cfg, &envelope) else {
        if cfg.mode == GateMode::Enforce {
            eprintln!("cacogate: built-in policy failed to compile; refusing");
            return REFUSED;
        }
        return exec(cfg, program, argv, &session, &tool_use_id, None);
    };
    match enforcement(g.verdict, g.mode) {
        Enforcement::Defer => exec(cfg, program, argv, &session, &tool_use_id, g.decision_id),
        e @ (Enforcement::Ask | Enforcement::Deny) => {
            eprintln!("{}", g.packet);
            if e == Enforcement::Ask {
                eprintln!(
                    "  note    this needs a person's approval and `cacogate run` has no one to ask; \
                     it was not run"
                );
            }
            record(
                cfg,
                &ExecutionRecord {
                    contract_version: CONTRACT_VERSION.into(),
                    session_id: session,
                    tool_use_id,
                    decision_id: g.decision_id,
                    executed: false,
                    mode: None,
                    exit_code: Some(i64::from(REFUSED)),
                    is_error: Some(true),
                    bucket: Bucket::Refusal,
                    duration_ms: None,
                    ts: now_rfc3339(),
                },
            );
            REFUSED
        }
    }
}

fn exec(
    cfg: &Config,
    program: &str,
    argv: &[String],
    session: &str,
    tool_use_id: &str,
    decision_id: Option<String>,
) -> i32 {
    let started = Instant::now();
    let status = Command::new(program)
        .args(argv.get(1..).unwrap_or_default())
        .status();
    let (code, bucket) = match &status {
        Ok(s) => (exit_code(*s), Bucket::Result),
        Err(e) => {
            eprintln!("cacogate run: cannot start {program}: {e}");
            (127, Bucket::ApparatusFailure)
        }
    };
    record(
        cfg,
        &ExecutionRecord {
            contract_version: CONTRACT_VERSION.into(),
            session_id: session.to_owned(),
            tool_use_id: tool_use_id.to_owned(),
            decision_id,
            executed: status.is_ok(),
            mode: Some(ExecMode::Direct),
            exit_code: Some(i64::from(code)),
            is_error: Some(code != 0),
            bucket,
            duration_ms: u64::try_from(started.elapsed().as_millis()).ok(),
            ts: now_rfc3339(),
        },
    );
    code
}

fn exit_code(s: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = s.signal() {
            return 128 + sig;
        }
    }
    s.code().unwrap_or(1)
}

fn record(cfg: &Config, rec: &ExecutionRecord) {
    let client = Client::new(&cfg.addr, Duration::from_secs(2));
    if let Err(e) = client.post::<_, serde_json::Value>("/v1/execution", rec) {
        gate::spool(cfg, "execution_not_recorded", &e.to_string());
    }
}

/// Session id: `$CACOGATE_SESSION`, else `<harness>-<pid>` of the nearest
/// ancestor that is not a shell. The harness spawns `sh -c "cacogate run …"`
/// and the shell may or may not exec us in place, so the parent alone is not
/// stable; the first non-shell ancestor is the harness process.
fn session_id(harness: HarnessName) -> String {
    if let Ok(s) = std::env::var("CACOGATE_SESSION")
        && !s.is_empty()
    {
        return s;
    }
    let name = serde_json::to_value(harness)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "other".into());
    format!("{name}-{}", harness_pid())
}

#[cfg(target_os = "linux")]
fn harness_pid() -> u32 {
    let mut pid = std::os::unix::process::parent_id();
    for _ in 0..8 {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            break;
        };
        // "pid (comm) state ppid …": comm may contain spaces and parens.
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
            break;
        };
        let comm = stat.get(open + 1..close).unwrap_or_default();
        if !matches!(comm, "sh" | "bash" | "dash" | "zsh" | "ash" | "busybox") {
            return pid;
        }
        let Some(ppid) = stat
            .get(close + 1..)
            .and_then(|r| r.split_whitespace().nth(1))
            .and_then(|p| p.parse().ok())
        else {
            break;
        };
        if ppid <= 1 {
            break;
        }
        pid = ppid;
    }
    pid
}

#[cfg(not(target_os = "linux"))]
fn harness_pid() -> u32 {
    std::os::unix::process::parent_id()
}
