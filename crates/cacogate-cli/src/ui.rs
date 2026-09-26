//! Terminal surfaces: watch, explain, status, doctor.
// `serde_json::Value[&str]` returns `Null` for missing keys and never panics;
// the lint cannot tell it apart from slice indexing.
#![allow(clippy::indexing_slicing)]

use crate::http::Client;
use cacogate_contracts::Verdict;
use cacogate_daemon::Config;
use cacogate_ledger::IndexRow;
use serde_json::Value;
use std::collections::HashSet;
use std::io::IsTerminal;
use std::time::Duration;

fn color_on() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn verdict_code(v: &str) -> &'static str {
    match v {
        "allow" => "32",
        "sandbox" | "canary" => "36",
        "revise" | "request_evidence" => "33",
        "ask_human" => "35",
        _ => "31",
    }
}

fn paint(s: &str, code: &str) -> String {
    if color_on() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_owned()
    }
}

/// Colour the first line of a packet by verdict.
pub(crate) fn paint_packet(p: &str, v: Verdict) -> String {
    let mut lines = p.lines();
    let first = lines.next().unwrap_or("");
    let mut out = paint(first, verdict_code(v.as_str()));
    for l in lines {
        out.push('\n');
        out.push_str(l);
    }
    out
}

/// Local UTC offset, read once in `main` before any thread exists (the
/// `time` crate refuses to read it later on Linux, where it is unsound).
static LOCAL_OFFSET: std::sync::OnceLock<time::UtcOffset> = std::sync::OnceLock::new();

/// Capture the local offset; call first thing in `main`.
pub(crate) fn init_local_offset() {
    if let Ok(o) = time::UtcOffset::current_local_offset() {
        let _ = LOCAL_OFFSET.set(o);
    }
}

/// `HH:MM:SS` in local time; the ledger itself stays in UTC. Falls back to
/// the UTC clock time with a `Z` when the offset is unknown.
fn local_hms(rfc3339: &str) -> String {
    let fmt = time::macros::format_description!("[hour]:[minute]:[second]");
    let parsed =
        time::OffsetDateTime::parse(rfc3339, &time::format_description::well_known::Rfc3339);
    match (parsed, LOCAL_OFFSET.get()) {
        (Ok(t), Some(off)) => t.to_offset(*off).format(&fmt).unwrap_or_default(),
        _ => format!("{}Z", rfc3339.get(11..19).unwrap_or(rfc3339)),
    }
}

fn row_line(r: &IndexRow) -> String {
    let time = local_hms(&r.recorded_at);
    let verdict = format!("{:<16}", r.verdict);
    let shadow = if r.mode == "shadow" && r.verdict != "allow" {
        " (shadow)"
    } else {
        ""
    };
    format!(
        "{time}  {}{shadow}  {:<11} {:<11} {}  {}",
        paint(&verdict, verdict_code(&r.verdict)),
        r.harness,
        r.class,
        r.summary,
        paint(&r.decision_id, "2")
    )
}

fn client(cfg: &Config) -> Client {
    Client::new(&cfg.addr, Duration::from_secs(3))
}

/// `cacogate watch`.
pub(crate) fn watch(cfg: &Config, once: bool, limit: usize) -> i32 {
    let c = client(cfg);
    let mut seen: HashSet<String> = HashSet::new();
    let mut first = true;
    loop {
        match c.get::<Vec<IndexRow>>(&format!(
            "/v1/decisions?limit={}",
            if first { limit } else { 200 }
        )) {
            Ok(rows) => {
                for r in rows {
                    if seen.insert(r.decision_id.clone()) {
                        println!("{}", row_line(&r));
                    }
                }
            }
            Err(e) => {
                eprintln!("cacogate: {e}");
                if once {
                    return 2;
                }
            }
        }
        if once {
            return 0;
        }
        first = false;
        std::thread::sleep(Duration::from_millis(700));
    }
}

/// `cacogate explain <id>`.
pub(crate) fn explain(cfg: &Config, id: &str) -> i32 {
    let v: Value = match client(cfg).get(&format!("/v1/decisions/{id}")) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cacogate: {e}");
            return 2;
        }
    };
    let row: Option<IndexRow> = serde_json::from_value(v["row"].clone()).ok();
    if let Some(r) = &row {
        println!("{}", row_line(r));
        println!(
            "  session {} · seq {} · tool {} · {}",
            r.session_id, r.seq, r.tool, r.recorded_at
        );
    }
    for e in v["entries"].as_array().into_iter().flatten() {
        let kind = e["kind"].as_str().unwrap_or("");
        let body = &e["body"];
        match kind {
            "decision" => {
                let d = &body["decision"];
                println!(
                    "\nverdict  {}   latency {} µs   policy {}",
                    d["verdict"].as_str().unwrap_or(""),
                    d["latency_us"],
                    d["policy_digest"]
                        .as_str()
                        .unwrap_or("")
                        .get(..19)
                        .unwrap_or("")
                );
                if let Some(r) = d["rationale"].as_str() {
                    println!("why      {r}");
                }
                println!("checks");
                for c in d["checks"].as_array().into_iter().flatten() {
                    let refs = c["evidence_refs"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    println!(
                        "  {:<11} {:<15} {:<17} allows {:<16} {}{}",
                        c["pillar"].as_str().unwrap_or(""),
                        c["id"].as_str().unwrap_or(""),
                        c["status"].as_str().unwrap_or(""),
                        c["allows"].as_str().unwrap_or(""),
                        c["detail"].as_str().unwrap_or(""),
                        if refs.is_empty() {
                            String::new()
                        } else {
                            format!(" [{refs}]")
                        }
                    );
                }
                if let Some(c) = d["risk"]["conformal"].as_object() {
                    println!(
                        "risk     stratum {} · calibration {}",
                        c.get("stratum").and_then(Value::as_str).unwrap_or(""),
                        c.get("status").and_then(Value::as_str).unwrap_or("")
                    );
                }
                for w in d["what_would_change_it"].as_array().into_iter().flatten() {
                    println!("change   {}", w.as_str().unwrap_or(""));
                }
                println!("digest   {}", d["decision_digest"].as_str().unwrap_or(""));
            }
            "envelope" => {
                println!("args     {}", body["args"]);
            }
            "execution" => {
                println!(
                    "executed {} · error {} · exit {} · {}",
                    body["executed"],
                    body["is_error"],
                    body["exit_code"],
                    body["ts"].as_str().unwrap_or("")
                );
            }
            _ => {}
        }
    }
    0
}

/// `cacogate status`.
pub(crate) fn status(cfg: &Config) -> i32 {
    match client(cfg).get::<Value>("/v1/status") {
        Ok(s) => {
            println!(
                "mode {} · policy {} · {} decisions · {} would hold · executions joined {}",
                s["mode"].as_str().unwrap_or(""),
                s["policy"].as_str().unwrap_or(""),
                s["total"],
                s["would_hold"],
                s["executions_joined"]
            );
            println!(
                "latency p50 {} µs · p95 {} µs (since daemon start)",
                s["p50_latency_us"], s["p95_latency_us"]
            );
            for key in ["by_verdict", "by_class", "by_harness"] {
                let parts: Vec<String> = s[key]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect();
                println!(
                    "{:<11} {}",
                    key.trim_start_matches("by_"),
                    parts.join(" · ")
                );
            }
            0
        }
        Err(e) => {
            eprintln!("cacogate: {e}");
            2
        }
    }
}

fn which(bin: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(bin))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

fn file_mentions(path: &std::path::Path, needle: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|t| t.contains(needle))
}

/// Whether any of `dirs` holds a cacogate adapter bundle (`.js` or `.ts`).
fn plugin_in(dirs: &[std::path::PathBuf]) -> bool {
    dirs.iter()
        .any(|d| d.join("cacogate.js").exists() || d.join("cacogate.ts").exists())
}

/// `cacogate doctor`.
pub(crate) fn doctor(cfg: &Config) -> i32 {
    let ok = |b: bool| {
        if b {
            paint("ok  ", "32")
        } else {
            paint("--  ", "33")
        }
    };
    println!(
        "config    {}",
        cfg.source.as_ref().map_or_else(
            || "(defaults; no config file)".into(),
            |p| p.display().to_string()
        )
    );
    println!("mode      {:?}", cfg.mode);
    let health = client(cfg).get::<Value>("/v1/health");
    match &health {
        Ok(h) => println!(
            "{}daemon  {} · policy {} · up {}s",
            ok(true),
            cfg.addr,
            h["policy"].as_str().unwrap_or(""),
            h["uptime_s"]
        ),
        Err(e) => println!("{}daemon  {e}", ok(false)),
    }
    let data_ok = std::fs::create_dir_all(&cfg.data_dir).is_ok();
    println!("{}ledger  {}", ok(data_ok), cfg.data_dir.display());
    let spool = cfg.data_dir.join("spool.jsonl");
    if let Ok(t) = std::fs::read_to_string(&spool) {
        println!(
            "    degradations spooled: {} (see {})",
            t.lines().count(),
            spool.display()
        );
    }
    let home = std::path::PathBuf::from(&cfg.home);
    let cwd = std::env::current_dir().unwrap_or_default();
    println!("harnesses");
    let claude_hook = [
        home.join(".claude/settings.json"),
        cwd.join(".claude/settings.json"),
        cwd.join(".claude/settings.local.json"),
    ]
    .iter()
    .any(|p| file_mentions(p, "hook claude-code"));
    for (name, bin, hooked, how) in [
        (
            "Claude Code",
            "claude",
            Some(claude_hook),
            "cacogate install claude-code --write",
        ),
        (
            "OpenCode",
            "opencode",
            Some(plugin_in(&[
                cwd.join(".opencode/plugin"),
                home.join(".config/opencode/plugin"),
            ])),
            "cp adapters/dist/opencode/cacogate.js .opencode/plugin/",
        ),
        (
            "pi",
            "pi",
            Some(plugin_in(&[
                cwd.join(".pi/extensions"),
                home.join(".pi/agent/extensions"),
            ])),
            "cp adapters/dist/pi/cacogate.js .pi/extensions/",
        ),
        (
            "omp",
            "omp",
            Some(plugin_in(&[
                cwd.join(".omp/extensions"),
                home.join(".omp/agent/extensions"),
                cwd.join(".pi/extensions"),
            ])),
            "omp -e adapters/dist/pi/cacogate.js (or copy into .omp/extensions/)",
        ),
        (
            "gnomon",
            "gnomon",
            Some(file_mentions(
                &cwd.join(".gnomon/roles.toml"),
                "cacogate run",
            )),
            "degraded path: see adapters/gnomon/README.md",
        ),
        ("codex", "codex", None, "adapter planned (P4)"),
    ] {
        let found = which(bin);
        let state = match hooked {
            Some(true) => "gate installed".to_owned(),
            Some(false) => format!("not gated: {how}"),
            None => how.to_owned(),
        };
        println!(
            "  {}{:<12} {:<40} {}",
            ok(found.is_some()),
            name,
            found.unwrap_or_else(|| "not on PATH".into()),
            state
        );
    }
    println!("sandbox backends (verdict not enforced yet: P1)");
    for b in ["bwrap", "docker", "nemoclaw"] {
        println!("  {}{b}", ok(which(b).is_some()));
    }
    i32::from(health.is_err())
}
