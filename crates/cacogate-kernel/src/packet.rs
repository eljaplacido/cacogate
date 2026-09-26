//! The decision packet: a short, templated summary shown to the agent and
//! the person. No model-written prose; every line derives from decision
//! fields.

use crate::facts::ActionFacts;
use cacogate_contracts::{CheckStatus, GateDecision, Verdict};

/// Display label for a verdict.
pub fn label(v: Verdict) -> &'static str {
    match v {
        Verdict::Allow => "ALLOW",
        Verdict::Sandbox => "SANDBOX",
        Verdict::Canary => "CANARY",
        Verdict::Revise => "REVISE",
        Verdict::RequestEvidence => "REQUEST EVIDENCE",
        Verdict::AskHuman => "ASK HUMAN",
        Verdict::Block => "BLOCK",
    }
}

/// Render the packet, at most six lines.
pub fn render(d: &GateDecision, f: &ActionFacts) -> String {
    let mut lines = vec![format!(
        "cacogate ▸ {}  (rung {}/6)   {}",
        label(d.verdict),
        d.verdict.rung(),
        f.summary
    )];
    let failing: Vec<_> = d
        .checks
        .iter()
        .filter(|c| matches!(c.status, CheckStatus::Failed | CheckStatus::CouldNotMeasure))
        .collect();
    if let Some(top) = failing.first() {
        let ev = if top.evidence_refs.is_empty() {
            String::new()
        } else {
            format!(" ({})", top.evidence_refs.join(", "))
        };
        lines.push(format!(
            "  why     {}: {}{}",
            top.id,
            top.detail.as_deref().unwrap_or(""),
            ev
        ));
    }
    if failing.len() > 1 {
        let rest: Vec<&str> = failing.iter().skip(1).map(|c| c.id.as_str()).collect();
        lines.push(format!("  also    {}", rest.join(", ")));
    }
    if let Some(c) = d.risk.as_ref().and_then(|r| r.conformal.as_ref()) {
        lines.push(format!(
            "  risk    {}: calibration {}",
            c.stratum.as_str(),
            serde_json::to_value(c.status)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        ));
    }
    if let Some(ob) = d.obligations.first() {
        let cmd = ob.command.as_ref().map_or_else(
            || "the test suite".to_owned(),
            |c| format!("`{}`", c.join(" ")),
        );
        lines.push(format!("  do      run {cmd}, then retry the same command"));
    } else if let Some(fix) = d.what_would_change_it.first() {
        lines.push(format!("  do      {fix}"));
    }
    lines.push(format!(
        "  id      {}   (cacogate explain {})",
        d.decision_id, d.decision_id
    ));
    lines.truncate(6);
    lines.join("\n")
}
