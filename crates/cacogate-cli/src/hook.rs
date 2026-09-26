//! `cacogate hook claude-code`: the Claude Code command hook.
//!
//! Contract with Claude Code: exit 0 + JSON on stdout for structured
//! decisions; exit 2 always blocks; any other exit code lets the action
//! proceed. So every failure path here is explicit: in enforce mode an
//! unreadable payload exits 2, and an unreachable daemon falls back to the
//! local kernel combined with the fail-class table. In shadow mode the hook
//! never interferes, but fallbacks are written to `spool.jsonl`.

use crate::gate;
use crate::http::Client;
use cacogate_contracts::{GateMode, Verdict};
use cacogate_daemon::Config;
use cacogate_daemon::engine::now_rfc3339;
use cacogate_harness::{Enforcement, claude, enforcement};
use serde_json::Value;
use std::io::{Read, Write};
use std::time::Duration;

const MAX_PAYLOAD: u64 = 32 * 1024 * 1024;

/// Run the hook; returns the process exit code.
pub(crate) fn run(cfg: &Config) -> i32 {
    let mut raw = String::new();
    if std::io::stdin()
        .take(MAX_PAYLOAD)
        .read_to_string(&mut raw)
        .is_err()
    {
        return unreadable(cfg, "stdin is not valid UTF-8");
    }
    let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
        return unreadable(cfg, "hook payload is not JSON");
    };
    match claude::event_name(&payload) {
        "PreToolUse" => pre(cfg, &payload),
        "PostToolUse" | "PostToolUseFailure" => post(cfg, &payload),
        _ => 0,
    }
}

fn unreadable(cfg: &Config, why: &str) -> i32 {
    gate::spool(cfg, "payload_unreadable", why);
    if cfg.mode == GateMode::Enforce {
        eprintln!("cacogate: {why}; blocking because the action cannot be identified");
        2
    } else {
        0
    }
}

fn pre(cfg: &Config, payload: &Value) -> i32 {
    let envelope = match claude::pre_to_envelope(payload, "unknown", &now_rfc3339()) {
        Ok(e) => e,
        Err(e) => return unreadable(cfg, &e.to_string()),
    };
    let Some(g) = gate::decide(cfg, &envelope) else {
        return if cfg.mode == GateMode::Enforce { 2 } else { 0 };
    };
    respond(enforcement(g.verdict, g.mode), g.verdict, &g.packet)
}

fn respond(e: Enforcement, verdict: Verdict, text: &str) -> i32 {
    match claude::pre_response(e, text) {
        None => 0,
        Some(v) => {
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{v}").is_err() {
                // Could not deliver structured output: exit 2 still blocks.
                return if verdict.executes() { 0 } else { 2 };
            }
            0
        }
    }
}

fn post(cfg: &Config, payload: &Value) -> i32 {
    let Ok(rec) = claude::post_to_execution(payload, &now_rfc3339()) else {
        gate::spool(
            cfg,
            "post_payload_unreadable",
            "missing session_id or tool_use_id",
        );
        return 0;
    };
    let client = Client::new(&cfg.addr, Duration::from_secs(2));
    if let Err(e) = client.post::<_, Value>("/v1/execution", &rec) {
        gate::spool(cfg, "execution_not_recorded", &e.to_string());
    }
    0
}
