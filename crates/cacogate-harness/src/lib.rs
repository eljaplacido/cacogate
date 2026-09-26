//! Harness adapters: translate native hook payloads to [`ActionEnvelope`]s
//! and decisions back to native responses.
//!
//! Only translation lives here. Adapters never compute risk (AGENTS.md
//! article 8); they send the raw action and render the reply.

pub mod claude;
pub mod repo;
pub mod wrap;

use cacogate_contracts::{GateDecision, GateMode, Verdict};

/// Adapter version stamped on envelopes.
pub const ADAPTER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Reply from `POST /v1/check`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckResponse {
    /// The decision.
    pub decision: GateDecision,
    /// Rendered packet.
    pub packet: String,
}

/// What the harness should be told, independent of harness wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enforcement {
    /// Say nothing; the harness's own permission flow applies.
    Defer,
    /// Ask the person through the native prompt.
    Ask,
    /// Refuse, with the packet as the reason.
    Deny,
}

/// Map a verdict to an enforcement under a mode. Shadow mode always defers.
///
/// Sandbox and canary are not implemented yet (P1). Until they are, an action
/// that would need confinement goes to a person rather than running unconfined.
pub fn enforcement(verdict: Verdict, mode: GateMode) -> Enforcement {
    if mode == GateMode::Shadow {
        return Enforcement::Defer;
    }
    match verdict {
        Verdict::Allow => Enforcement::Defer,
        Verdict::Sandbox | Verdict::Canary | Verdict::AskHuman => Enforcement::Ask,
        Verdict::Revise | Verdict::RequestEvidence | Verdict::Block => Enforcement::Deny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ad001_shadow_never_interferes() {
        for v in Verdict::LADDER {
            assert_eq!(enforcement(v, GateMode::Shadow), Enforcement::Defer);
        }
    }

    #[test]
    fn test_ad002_enforce_mapping_never_runs_held_actions() {
        for v in Verdict::LADDER {
            let e = enforcement(v, GateMode::Enforce);
            if !v.executes() {
                assert_ne!(e, Enforcement::Defer, "{v:?} must not run silently");
            }
        }
        assert_eq!(
            enforcement(Verdict::Allow, GateMode::Enforce),
            Enforcement::Defer
        );
    }
}
