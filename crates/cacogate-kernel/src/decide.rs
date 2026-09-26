//! Checks → verdict. The combination rule is monotone: the decision is the
//! most restrictive `allows` over all checks, so adding a check can only
//! lower autonomy.

use crate::facts::ActionFacts;
use crate::policy::CompiledPolicy;
use crate::session::SessionState;
use crate::shell::ParseStatus;
use cacogate_contracts::{
    ActionEnvelope, CONTRACT_VERSION, CalibrationStatus, Check, CheckStatus, ConformalRisk,
    GateDecision, GateMode, Obligation, ObligationKind, Pillar, Risk, Verdict,
};

/// Everything `decide` needs. The daemon supplies ids and the clock.
#[derive(Debug)]
pub struct DecideInput<'a> {
    /// Envelope.
    pub envelope: &'a ActionEnvelope,
    /// Facts extracted from it.
    pub facts: &'a ActionFacts,
    /// Session history.
    pub session: &'a SessionState,
    /// Policy.
    pub policy: &'a CompiledPolicy,
    /// Shadow or enforce.
    pub mode: GateMode,
    /// Decision id.
    pub decision_id: String,
    /// Digest of the envelope.
    pub envelope_digest: String,
}

/// The weakest-allowed combination: the most restrictive `allows`.
///
/// This is a pure maximum so that adding a check can never loosen a
/// decision. The "nothing measured is not an allow" rule is enforced
/// structurally instead: [`decide`] always emits a measured provenance
/// check, and an empty slice yields `unmeasured`.
pub fn combine(checks: &[Check], unmeasured: Verdict) -> Verdict {
    checks.iter().map(|c| c.allows).max().unwrap_or(unmeasured)
}

/// Decide on one envelope. Pure.
pub fn decide(input: &DecideInput<'_>) -> GateDecision {
    let f = input.facts;
    let mut checks = Vec::new();

    // Provenance: did we understand the action?
    let unclear = matches!(
        f.parse_status,
        ParseStatus::Unparseable | ParseStatus::Partial
    ) || !f.unresolved_writes.is_empty();
    checks.push(if unclear {
        let mut notes = f.parse_notes.clone();
        notes.extend(
            f.unresolved_writes
                .iter()
                .map(|w| format!("unresolved write target {w}")),
        );
        Check {
            pillar: Pillar::Provenance,
            id: "PRV-PARSE-001".into(),
            status: CheckStatus::CouldNotMeasure,
            allows: f.class.unmeasured_verdict(),
            rule_class: None,
            detail: Some(format!(
                "could not fully understand the command: {}",
                notes.join("; ")
            )),
            evidence_refs: vec![],
        }
    } else {
        Check {
            pillar: Pillar::Provenance,
            id: "PRV-PARSE-001".into(),
            status: CheckStatus::Passed,
            allows: Verdict::Allow,
            rule_class: None,
            detail: None,
            evidence_refs: vec![],
        }
    });

    // Symbolic: every firing rule is its own check.
    let branch = input
        .envelope
        .repo
        .as_ref()
        .and_then(|r| r.branch.as_deref());
    let mut firings = input.policy.evaluate(f, input.session, branch);
    firings.sort_by(|a, b| b.verdict.cmp(&a.verdict));
    if firings.is_empty() {
        checks.push(Check {
            pillar: Pillar::Symbolic,
            id: "SYM-POLICY".into(),
            status: CheckStatus::Passed,
            allows: Verdict::Allow,
            rule_class: None,
            detail: Some(format!(
                "{} rules, none fired",
                input.policy.policy.rules.len()
            )),
            evidence_refs: vec![],
        });
    }
    let mut obligations = Vec::new();
    let mut remedies = Vec::new();
    for fr in &firings {
        checks.push(Check {
            pillar: Pillar::Symbolic,
            id: fr.rule.id.clone(),
            status: CheckStatus::Failed,
            allows: fr.verdict,
            rule_class: Some(fr.rule.class),
            detail: Some(fr.rule.reason.clone()),
            evidence_refs: fr.evidence.clone(),
        });
        remedies.push(fr.rule.remedy.clone());
        if fr.verdict == Verdict::RequestEvidence {
            obligations.push(Obligation {
                kind: ObligationKind::Run,
                command: input.policy.policy.test_commands.first().cloned(),
                must_follow_seq: input.session.last_edit_seq,
            });
        }
    }

    let verdict = combine(&checks, f.class.unmeasured_verdict());
    let rationale = firings
        .first()
        .map(|fr| format!("{}: {}", fr.rule.id, fr.rule.reason))
        .or_else(|| {
            checks
                .iter()
                .find(|c| c.status == CheckStatus::CouldNotMeasure)
                .and_then(|c| c.detail.clone())
        });

    GateDecision {
        contract_version: CONTRACT_VERSION.into(),
        decision_id: input.decision_id.clone(),
        envelope_digest: input.envelope_digest.clone(),
        verdict,
        mode: Some(input.mode),
        checks,
        obligations,
        constraints: vec![],
        rationale,
        what_would_change_it: remedies,
        risk: Some(Risk {
            // The conformal module is not built yet; say so rather than omit it.
            conformal: Some(ConformalRisk {
                stratum: f.class,
                mode_set: vec![],
                alpha_target: None,
                status: CalibrationStatus::Absent,
                calibration_id: None,
                n_cal: None,
            }),
            causal: None,
        }),
        policy_digest: input.policy.digest.clone(),
        calibration_digest: None,
        capability: None,
        degradations: vec![],
        latency_us: 0,
        decision_digest: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{
        extract,
        tests::{env_for, kenv},
    };
    use crate::policy::Policy;
    use cacogate_contracts::{ActionClass, RuleClass};
    use proptest::prelude::*;
    use serde_json::json;

    fn run(tool: &str, args: serde_json::Value) -> GateDecision {
        let env = env_for(tool, args);
        let facts = extract(&env, &kenv());
        let policy = Policy::builtin_strict().compile().unwrap();
        decide(&DecideInput {
            envelope: &env,
            facts: &facts,
            session: &SessionState::default(),
            policy: &policy,
            mode: GateMode::Shadow,
            decision_id: "d".into(),
            envelope_digest: "sha256:x".into(),
        })
    }

    #[test]
    fn test_de001_benign_read_is_allowed() {
        assert_eq!(
            run("Bash", json!({"command": "ls"})).verdict,
            Verdict::Allow
        );
        assert_eq!(
            run("Read", json!({"file_path": "/repo/a"})).verdict,
            Verdict::Allow
        );
    }

    #[test]
    fn test_de002_invariant_blocks() {
        let d = run("Bash", json!({"command": "git push --force origin main"}));
        assert_eq!(d.verdict, Verdict::Block);
        assert_eq!(
            d.rationale.as_deref().map(|r| r.starts_with("INV-VCS-001")),
            Some(true)
        );
    }

    #[test]
    fn test_de003_unparseable_mutation_asks_human() {
        let d = run("Bash", json!({"command": "rm 'unterminated"}));
        assert_eq!(d.verdict, Verdict::AskHuman);
        assert!(
            d.checks
                .iter()
                .any(|c| c.status == CheckStatus::CouldNotMeasure)
        );
    }

    #[test]
    fn test_de004_self_protection_blocks_file_tools() {
        let d = run(
            "Write",
            json!({"file_path": "/repo/.claude/settings.json", "content": "{}"}),
        );
        assert_eq!(d.verdict, Verdict::Block);
    }

    #[test]
    fn test_de005_conformal_status_is_reported_absent() {
        let d = run("Bash", json!({"command": "cargo build"}));
        let c = d.risk.and_then(|r| r.conformal).unwrap();
        assert_eq!(c.status, CalibrationStatus::Absent);
        assert_eq!(c.stratum, ActionClass::ShellLocal);
    }

    #[test]
    fn test_de006_decisions_always_carry_a_measured_check() {
        assert_eq!(combine(&[], Verdict::AskHuman), Verdict::AskHuman);
        for cmd in ["ls", "rm 'x", "git push", "eval $X"] {
            let d = run("Bash", json!({"command": cmd}));
            assert!(
                d.checks
                    .iter()
                    .any(|c| c.pillar == Pillar::Provenance
                        && c.status != CheckStatus::NotApplicable)
            );
        }
    }

    fn arb_verdict() -> impl Strategy<Value = Verdict> {
        (0usize..7).prop_map(|i| Verdict::LADDER[i])
    }

    fn check(v: Verdict) -> Check {
        Check {
            pillar: Pillar::Symbolic,
            id: "p".into(),
            status: CheckStatus::Failed,
            allows: v,
            rule_class: Some(RuleClass::Defeasible),
            detail: None,
            evidence_refs: vec![],
        }
    }

    proptest! {
        #[test]
        fn test_de007_adding_a_check_never_loosens(vs in proptest::collection::vec(arb_verdict(), 1..8), extra in arb_verdict()) {
            let base: Vec<Check> = vs.iter().copied().map(check).collect();
            let before = combine(&base, Verdict::AskHuman);
            let mut more = base.clone();
            more.push(check(extra));
            prop_assert!(combine(&more, Verdict::AskHuman) >= before);
        }
    }
}
