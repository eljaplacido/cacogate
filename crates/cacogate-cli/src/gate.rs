//! One decision path for every CLI adapter: ask the daemon, and when it is
//! unreachable decide locally with the same kernel, never looser than the
//! fail-class table (AGENTS.md article 5).

use crate::http::Client;
use cacogate_contracts::{ActionEnvelope, Check, CheckStatus, GateMode, Pillar, Verdict};
use cacogate_daemon::Config;
use cacogate_daemon::engine::now_rfc3339;
use cacogate_harness::CheckResponse;
use cacogate_kernel::{
    DecideInput, KernelEnv, SessionState, combine, decide as kdecide, extract, packet,
};
use serde_json::json;
use std::io::Write;
use std::time::Duration;

/// A decision as an adapter needs it.
pub(crate) struct Gated {
    pub(crate) verdict: Verdict,
    pub(crate) mode: GateMode,
    pub(crate) packet: String,
    /// `None` when decided locally (nothing in the ledger to join to).
    pub(crate) decision_id: Option<String>,
}

/// Decide on an envelope. `None` means the built-in policy failed to
/// compile; the caller must fail closed.
pub(crate) fn decide(cfg: &Config, envelope: &ActionEnvelope) -> Option<Gated> {
    let client = Client::new(&cfg.addr, Duration::from_secs(3));
    let err = match client.post::<_, CheckResponse>("/v1/check", envelope) {
        Ok(r) => {
            return Some(Gated {
                verdict: r.decision.verdict,
                mode: r.decision.mode.unwrap_or(cfg.mode),
                packet: r.packet,
                decision_id: Some(r.decision.decision_id),
            });
        }
        Err(e) => e,
    };
    // Local fallback: same kernel, builtin policy, no session history.
    spool(cfg, "daemon_unreachable", &err.to_string());
    let compiled = cfg.policy().compile().ok()?;
    let facts = extract(
        envelope,
        &KernelEnv {
            home: cfg.home.clone(),
        },
    );
    let mut d = kdecide(&DecideInput {
        envelope,
        facts: &facts,
        session: &SessionState::default(),
        policy: &compiled,
        mode: cfg.mode,
        decision_id: "local".into(),
        envelope_digest: String::new(),
    });
    // The fault is itself a check, so the packet says why and the
    // combination stays the monotone maximum.
    d.checks.insert(
        0,
        Check {
            pillar: Pillar::Provenance,
            id: "PRV-GATE-001".into(),
            status: CheckStatus::CouldNotMeasure,
            allows: facts.class.fail_verdict(),
            rule_class: None,
            detail: Some(format!(
                "cacogate daemon unreachable; fail-safe default for {} actions, no session history",
                facts.class.as_str()
            )),
            evidence_refs: vec![],
        },
    );
    d.verdict = combine(&d.checks, facts.class.unmeasured_verdict());
    Some(Gated {
        verdict: d.verdict,
        mode: cfg.mode,
        packet: packet::render(&d, &facts),
        decision_id: None,
    })
}

/// Append a degradation line to `<data_dir>/spool.jsonl` (best effort).
pub(crate) fn spool(cfg: &Config, id: &str, detail: &str) {
    let line = json!({ "ts": now_rfc3339(), "id": id, "detail": detail });
    if std::fs::create_dir_all(&cfg.data_dir).is_ok()
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(cfg.data_dir.join("spool.jsonl"))
    {
        let _ = writeln!(f, "{line}");
    }
}
