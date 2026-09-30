//! `cacogate corpus <file>`: replay recorded agent tool calls through the
//! kernel and report what the gate would have done to real traffic.
//!
//! This answers the question hand-written fixtures cannot: a rule that *can*
//! fire (PO-002) is not a rule that *will* fire on what agents actually do,
//! and a rule that fires on harmless commands is a cost only real traffic
//! reveals. It is also the offline action-rate meter: the would-refuse rate
//! on past sessions, before the gate is installed anywhere. Each row is
//! decided alone (no session history), so history rules are not exercised.

use cacogate_contracts::{
    ActionEnvelope, CONTRACT_VERSION, CheckStatus, GateMode, Harness, HarnessName, Tool,
    action_digest,
};
use cacogate_daemon::Config;
use cacogate_harness::repo;
use cacogate_kernel::{DecideInput, SessionState, decide, extract};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};

/// One recorded tool call (`scripts/harvest_agent_commands.py` writes these).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Row {
    pub(crate) tool: String,
    pub(crate) args: Map<String, Value>,
    pub(crate) cwd: String,
    #[serde(default)]
    pub(crate) source: Option<String>,
    #[serde(default)]
    pub(crate) harness_version: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
}

/// Replay summary.
#[derive(Debug, Default, Serialize)]
pub(crate) struct Report {
    pub(crate) rows: u64,
    pub(crate) skipped: u64,
    pub(crate) would_hold: u64,
    /// Held although no rule fired: the kernel could not measure the action
    /// (e.g. a write to `$VAR/…`), so the fail-safe default applied.
    pub(crate) held_unmeasured: u64,
    pub(crate) by_verdict: BTreeMap<String, u64>,
    pub(crate) by_class: BTreeMap<String, u64>,
    pub(crate) by_parse: BTreeMap<String, u64>,
    pub(crate) by_rule: BTreeMap<String, u64>,
    pub(crate) gate_reads: u64,
    pub(crate) examples: BTreeMap<String, Vec<String>>,
}

/// Replay rows; `fired_out` receives every row on which a rule fired, with
/// the rules, for labelling.
pub(crate) fn replay(
    cfg: &Config,
    rows: impl Iterator<Item = Option<Row>>,
    examples: usize,
    mut fired_out: Option<&mut dyn Write>,
) -> Result<Report, String> {
    let policy = cfg
        .policy()
        .compile()
        .map_err(|e| format!("policy failed to compile: {e}"))?;
    let mut repos: HashMap<String, Option<cacogate_contracts::Repo>> = HashMap::new();
    let mut r = Report::default();
    for (i, row) in rows.enumerate() {
        let Some(row) = row else {
            r.skipped += 1;
            continue;
        };
        let repo = repos
            .entry(row.cwd.clone())
            .or_insert_with(|| repo::probe(&row.cwd))
            .clone();
        let root = repo.as_ref().and_then(|x| x.root.clone());
        let Ok(digest) = action_digest(&row.tool, &row.args, &row.cwd, root.as_deref()) else {
            r.skipped += 1;
            continue;
        };
        let env = ActionEnvelope {
            contract_version: CONTRACT_VERSION.into(),
            envelope_id: format!("corpus-{i}"),
            harness: Harness {
                // Transcript rows say which harness issued them; the linked
                // roots (a harness's own state dir) depend on it.
                name: if row
                    .source
                    .as_deref()
                    .is_some_and(|s| s.starts_with("claude_code"))
                {
                    HarnessName::ClaudeCode
                } else {
                    HarnessName::Other
                },
                version: row
                    .harness_version
                    .clone()
                    .unwrap_or_else(|| "unknown".into()),
                adapter_version: env!("CARGO_PKG_VERSION").into(),
                interactive: Some(false),
            },
            session_id: format!("corpus-{i}"),
            turn_id: None,
            tool_use_id: format!("corpus-{i}"),
            agent: None,
            model: None,
            tool: Tool {
                raw_name: row.tool.clone(),
                kind: None,
            },
            args: row.args.clone(),
            cwd: row.cwd.clone(),
            repo,
            surface_digest: None,
            ts: "1970-01-01T00:00:00Z".into(),
            action_digest: digest,
        };
        let facts = extract(&env, &cacogate_daemon::engine::kernel_env(&cfg.home, &env));
        let d = decide(&DecideInput {
            envelope: &env,
            facts: &facts,
            session: &SessionState::default(),
            policy: &policy,
            mode: GateMode::Enforce,
            decision_id: format!("corpus-{i}"),
            envelope_digest: String::new(),
        });
        r.rows += 1;
        *r.by_verdict.entry(d.verdict.as_str().into()).or_default() += 1;
        *r.by_class.entry(facts.class.as_str().into()).or_default() += 1;
        let parse = serde_json::to_value(facts.parse_status)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        *r.by_parse.entry(parse).or_default() += 1;
        if !d.verdict.executes() {
            r.would_hold += 1;
        }
        r.gate_reads += u64::from(!policy.gate_reads(&facts).is_empty());
        let fired: Vec<String> = d
            .checks
            .iter()
            .filter(|c| c.status == CheckStatus::Failed)
            .map(|c| c.id.clone())
            .collect();
        if !d.verdict.executes() && fired.is_empty() {
            r.held_unmeasured += 1;
        }
        for id in &fired {
            *r.by_rule.entry(id.clone()).or_default() += 1;
            let ex = r.examples.entry(id.clone()).or_default();
            if ex.len() < examples {
                ex.push(facts.summary.clone());
            }
        }
        if !fired.is_empty()
            && let Some(w) = fired_out.as_deref_mut()
        {
            let line = json!({
                "tool": row.tool, "args": row.args, "cwd": row.cwd,
                "source": row.source, "fired": fired, "verdict": d.verdict.as_str(),
            });
            let _ = writeln!(w, "{line}");
        }
    }
    Ok(r)
}

/// Read JSON lines; a malformed line becomes `None` (counted as skipped).
pub(crate) fn read_rows(path: &str) -> Result<Vec<Option<Row>>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    Ok(BufReader::new(f)
        .lines()
        .map(|l| l.ok().and_then(|l| serde_json::from_str::<Row>(&l).ok()))
        .collect())
}

/// `cacogate corpus` entry point.
pub(crate) fn run(
    cfg: &Config,
    path: &str,
    as_json: bool,
    examples: usize,
    fired_out: Option<&str>,
) -> i32 {
    let rows = match read_rows(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cacogate corpus: {e}");
            return 2;
        }
    };
    let mut sink = match fired_out.map(std::fs::File::create).transpose() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cacogate corpus: {e}");
            return 2;
        }
    };
    let report = match replay(
        cfg,
        rows.into_iter(),
        examples,
        sink.as_mut().map(|f| f as &mut dyn Write),
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cacogate corpus: {e}");
            return 2;
        }
    };
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
        return 0;
    }
    let pct = |n: u64| {
        if report.rows == 0 {
            0.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            let p = n as f64 * 100.0 / report.rows as f64;
            p
        }
    };
    println!(
        "{} tool calls replayed ({} skipped) · would refuse {} ({:.2}%), of which {} unmeasured · read gate files {}",
        report.rows,
        report.skipped,
        report.would_hold,
        pct(report.would_hold),
        report.held_unmeasured,
        report.gate_reads
    );
    for (label, m) in [
        ("parse", &report.by_parse),
        ("verdict", &report.by_verdict),
        ("class", &report.by_class),
    ] {
        let parts: Vec<String> = m.iter().map(|(k, v)| format!("{k} {v}")).collect();
        println!("{label:<8} {}", parts.join(" · "));
    }
    let mut rules: Vec<_> = report.by_rule.iter().collect();
    rules.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
    println!("rules that fired (a failed check), with share of all calls:");
    for (id, n) in rules {
        println!("  {id:<18} {n:>6}  {:>6.2}%", pct(*n));
        for ex in report.examples.get(id).into_iter().flatten() {
            let ex: String = ex.chars().take(110).collect();
            println!("      e.g. {ex}");
        }
    }
    0
}
