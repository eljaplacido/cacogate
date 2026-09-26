//! cacogate wire contracts, version 0.1.0.
//!
//! These types mirror `contracts/v0/*.schema.json` exactly; the schemas are
//! the source of truth and `tests/schema_roundtrip.rs` checks that every value
//! these types produce validates. All structs reject unknown fields.

pub mod canon;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Contract version stamped on every envelope and decision.
pub const CONTRACT_VERSION: &str = "0.1.0";

/// The verdict ladder. Declaration order is the rung order, so `Ord` is the
/// "more restrictive than" relation that the combination rule relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Defer to the harness's own permission flow.
    Allow,
    /// Execute in a declared confinement backend.
    Sandbox,
    /// Execute against a limited target and watch the follow-up.
    Canary,
    /// Do not execute; return constraints to the agent.
    Revise,
    /// Do not execute until named obligations are satisfied.
    RequestEvidence,
    /// Execute only after a person approves this exact action.
    AskHuman,
    /// Never execute.
    Block,
}

impl Verdict {
    /// All rungs, least to most restrictive.
    pub const LADDER: [Verdict; 7] = [
        Verdict::Allow,
        Verdict::Sandbox,
        Verdict::Canary,
        Verdict::Revise,
        Verdict::RequestEvidence,
        Verdict::AskHuman,
        Verdict::Block,
    ];

    /// Rung index, 0 (`allow`) to 6 (`block`).
    pub fn rung(self) -> u8 {
        match self {
            Verdict::Allow => 0,
            Verdict::Sandbox => 1,
            Verdict::Canary => 2,
            Verdict::Revise => 3,
            Verdict::RequestEvidence => 4,
            Verdict::AskHuman => 5,
            Verdict::Block => 6,
        }
    }

    /// Whether the action runs under this verdict.
    pub fn executes(self) -> bool {
        matches!(self, Verdict::Allow | Verdict::Sandbox | Verdict::Canary)
    }

    /// Wire name (`request_evidence`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::Sandbox => "sandbox",
            Verdict::Canary => "canary",
            Verdict::Revise => "revise",
            Verdict::RequestEvidence => "request_evidence",
            Verdict::AskHuman => "ask_human",
            Verdict::Block => "block",
        }
    }
}

/// Four-valued check outcome. `could_not_measure` is never a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// The check ran and found nothing against the action.
    Passed,
    /// The check ran and found a problem.
    Failed,
    /// The check does not apply to this action.
    NotApplicable,
    /// The check could not be evaluated.
    CouldNotMeasure,
}

/// Which assurance pillar produced a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pillar {
    /// Policy rules and session-history rules.
    Symbolic,
    /// Repository impact / identification.
    Causal,
    /// Calibrated risk.
    Conformal,
    /// Session and agent risk budget.
    Budget,
    /// Provenance / apparatus health (parsing, adapter coverage).
    Provenance,
}

/// Precedence class of a symbolic rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleClass {
    /// Cannot be overridden in-session; may block.
    Invariant,
    /// A person may override with a reason; at most `ask_human`.
    Defeasible,
    /// Informational; at most `revise`.
    Advisory,
}

impl RuleClass {
    /// Most restrictive verdict a rule of this class may produce.
    pub fn max_verdict(self) -> Verdict {
        match self {
            RuleClass::Invariant => Verdict::Block,
            RuleClass::Defeasible => Verdict::AskHuman,
            RuleClass::Advisory => Verdict::Revise,
        }
    }
}

/// Coarse action class; also the conformal stratum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    /// No side effects.
    Read,
    /// Edits inside the repository root.
    LocalEdit,
    /// Local command execution.
    ShellLocal,
    /// Local version-control mutation.
    VcsLocal,
    /// MCP tool call (unknown effects).
    Mcp,
    /// Delegation to a sub-agent.
    Delegate,
    /// Network access.
    Network,
    /// Mutation of a remote repository.
    VcsRemote,
    /// Deploy, publish or cloud mutation.
    Deploy,
}

impl ActionClass {
    /// All classes, in the order used by the golden fixtures.
    pub const ALL: [ActionClass; 9] = [
        ActionClass::Read,
        ActionClass::LocalEdit,
        ActionClass::ShellLocal,
        ActionClass::VcsLocal,
        ActionClass::VcsRemote,
        ActionClass::Network,
        ActionClass::Deploy,
        ActionClass::Mcp,
        ActionClass::Delegate,
    ];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            ActionClass::Read => "read",
            ActionClass::LocalEdit => "local_edit",
            ActionClass::ShellLocal => "shell_local",
            ActionClass::VcsLocal => "vcs_local",
            ActionClass::Mcp => "mcp",
            ActionClass::Delegate => "delegate",
            ActionClass::Network => "network",
            ActionClass::VcsRemote => "vcs_remote",
            ActionClass::Deploy => "deploy",
        }
    }

    /// Verdict when the gate itself cannot decide (daemon down, fault).
    /// Mirrors `conformance/verdicts.golden.json#fail_class`.
    pub fn fail_verdict(self) -> Verdict {
        match self {
            ActionClass::Read => Verdict::Allow,
            ActionClass::LocalEdit
            | ActionClass::ShellLocal
            | ActionClass::VcsLocal
            | ActionClass::Mcp
            | ActionClass::Delegate => Verdict::AskHuman,
            ActionClass::VcsRemote | ActionClass::Network | ActionClass::Deploy => Verdict::Block,
        }
    }

    /// Verdict contributed by a check that could not measure this class.
    pub fn unmeasured_verdict(self) -> Verdict {
        if self == ActionClass::Read {
            Verdict::Allow
        } else {
            Verdict::AskHuman
        }
    }
}

/// Harness identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessName {
    /// Anthropic Claude Code.
    ClaudeCode,
    /// `OpenCode`.
    Opencode,
    /// Upstream pi coding agent.
    Pi,
    /// oh-my-pi.
    Omp,
    /// The user's gnomon harness.
    Gnomon,
    /// `OpenAI` Codex CLI.
    Codex,
    /// Google Gemini CLI.
    Gemini,
    /// `OpenFang` runtime.
    Openfang,
    /// Anything else.
    Other,
}

/// Which harness sent the action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Harness {
    /// Harness identifier.
    pub name: HarnessName,
    /// Harness version, `unknown` if not reported.
    pub version: String,
    /// Adapter version.
    pub adapter_version: String,
    /// Whether a person can answer a native prompt right now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive: Option<bool>,
}

/// Agent identity inside a harness session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    /// Role or agent name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Parent agent / delegating call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

/// Model behind the agent, when the harness reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    /// Model identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Endpoint kind (`ollama`, `openai`, `anthropic`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_kind: Option<String>,
}

/// Tool kind hint from the adapter; the kernel re-derives its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// Shell command.
    Shell,
    /// File read.
    FileRead,
    /// Whole-file write.
    FileWrite,
    /// In-place edit.
    FileEdit,
    /// Glob / grep / find / ls.
    Search,
    /// Web fetch / search.
    NetFetch,
    /// MCP tool.
    Mcp,
    /// Sub-agent delegation.
    Delegate,
    /// Anything else.
    Other,
}

/// The tool being invoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// Harness-native tool name, e.g. `Bash`, `edit`, `mcp__x__y`.
    pub raw_name: String,
    /// Adapter's hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ToolKind>,
}

/// Repository context.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repo {
    /// Repository root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// HEAD commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// Current branch, when on one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Whether the working tree had uncommitted changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
}

/// What an adapter sends before a tool executes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionEnvelope {
    /// Always [`CONTRACT_VERSION`].
    pub contract_version: String,
    /// Unique per envelope.
    pub envelope_id: String,
    /// Sending harness.
    pub harness: Harness,
    /// Harness session id.
    pub session_id: String,
    /// Turn / prompt id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// Harness tool-call id.
    pub tool_use_id: String,
    /// Agent identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<Agent>,
    /// Model identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Model>,
    /// Tool.
    pub tool: Tool,
    /// Harness-native arguments.
    pub args: serde_json::Map<String, Value>,
    /// Working directory.
    pub cwd: String,
    /// Repository context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<Repo>,
    /// Harness surface/config digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_digest: Option<String>,
    /// RFC 3339 timestamp from the adapter.
    pub ts: String,
    /// `sha256(JCS({tool, args, cwd, root}))`, see [`action_digest`].
    pub action_digest: String,
}

/// The part of an action that identity (and approvals) bind to.
#[derive(Serialize)]
struct ActionIdentity<'a> {
    tool: &'a str,
    args: &'a serde_json::Map<String, Value>,
    cwd: &'a str,
    root: Option<&'a str>,
}

/// Compute the action digest: what approvals and capabilities bind to.
pub fn action_digest(
    raw_tool: &str,
    args: &serde_json::Map<String, Value>,
    cwd: &str,
    root: Option<&str>,
) -> Result<String, canon::CanonError> {
    canon::digest(&ActionIdentity {
        tool: raw_tool,
        args,
        cwd,
        root,
    })
}

impl ActionEnvelope {
    /// Recompute the action digest from this envelope's own fields.
    pub fn recompute_action_digest(&self) -> Result<String, canon::CanonError> {
        let root = self.repo.as_ref().and_then(|r| r.root.as_deref());
        action_digest(&self.tool.raw_name, &self.args, &self.cwd, root)
    }
}

/// One independently reported check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    /// Producing pillar.
    pub pillar: Pillar,
    /// Rule or check id.
    pub id: String,
    /// Four-valued status.
    pub status: CheckStatus,
    /// Least restrictive verdict this check still permits.
    pub allows: Verdict,
    /// Rule class, for symbolic checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_class: Option<RuleClass>,
    /// Human-readable detail (templated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// References into the ledger or repository.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
}

/// Kinds of evidence obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationKind {
    /// Run a command and have it succeed.
    Run,
    /// `septacore check` must exit 0.
    SeptacoreCheck,
    /// A sandboxed shadow run.
    ShadowRun,
    /// A person reviews the action.
    HumanReview,
    /// The agent provides missing context.
    ProvideContext,
}

/// Something that must be true before a `request_evidence` action may retry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Obligation {
    /// Kind.
    pub kind: ObligationKind,
    /// Command argv, for `run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// Evidence must be recorded after this ledger sequence number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub must_follow_seq: Option<u64>,
}

/// Whether the gate enforces or only records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GateMode {
    /// Record verdicts, always answer `allow`.
    #[default]
    Shadow,
    /// Enforce verdicts.
    Enforce,
}

/// A recorded fallback or reduced capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Degradation {
    /// Stable id, e.g. `daemon_unreachable`.
    pub id: String,
    /// Detail.
    pub detail: String,
}

/// Conformal risk summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConformalRisk {
    /// Stratum (action class).
    pub stratum: ActionClass,
    /// Permitted execution modes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mode_set: Vec<ExecMode>,
    /// Chosen target false-allow rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_target: Option<f64>,
    /// Calibration status.
    pub status: CalibrationStatus,
    /// Calibration artefact id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration_id: Option<String>,
    /// Calibration sample size in this stratum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_cal: Option<u64>,
}

/// Calibration artefact status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStatus {
    /// Valid artefact, enough data.
    Calibrated,
    /// Fewer than `n_min` labelled outcomes.
    InsufficientN,
    /// Binding digests no longer match.
    Stale,
    /// Online monitor detected distribution shift.
    Shifted,
    /// No artefact loaded.
    Absent,
}

/// Execution modes an action may run in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecMode {
    /// Natively.
    Direct,
    /// In a sandbox backend.
    Sandbox,
    /// Canary.
    Canary,
}

/// Blast radius from the repository impact graph.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlastRadius {
    /// Dependent files/modules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependents: Option<u64>,
    /// Covering tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<u64>,
    /// Services reachable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,
    /// Whether a public API is touched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_api_touched: Option<bool>,
    /// Code owners crossed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owners_crossed: Vec<String>,
}

/// Identification status of a causal estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Identification {
    /// Effect identified.
    Identified,
    /// Estimator abstained.
    Abstained,
    /// No estimate attempted.
    NotAttempted,
}

/// Causal impact summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CausalRisk {
    /// Blast radius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blast_radius: Option<BlastRadius>,
    /// Identification status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identification: Option<Identification>,
    /// Digest of the impact graph used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rig_digest: Option<String>,
}

/// Risk block of a decision.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Risk {
    /// Conformal part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conformal: Option<ConformalRisk>,
    /// Causal part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causal: Option<CausalRisk>,
}

/// Single-use permit bound to one action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    /// Unique id; consumed once.
    pub cap_id: String,
    /// The action it authorises.
    pub action_digest: String,
    /// Mode it authorises.
    pub mode: ExecMode,
    /// Expiry (RFC 3339).
    pub not_after: String,
    /// `hmac-sha256:<hex>`.
    pub mac: String,
}

/// The daemon's verdict for one envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateDecision {
    /// Always [`CONTRACT_VERSION`].
    pub contract_version: String,
    /// Unique id.
    pub decision_id: String,
    /// Digest of the envelope decided on.
    pub envelope_digest: String,
    /// Final verdict (most restrictive `allows`).
    pub verdict: Verdict,
    /// Shadow or enforce.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<GateMode>,
    /// Every check, passed or not.
    pub checks: Vec<Check>,
    /// Evidence obligations for `request_evidence`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub obligations: Vec<Obligation>,
    /// Constraints for `revise`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
    /// Templated rationale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    /// What would change the verdict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub what_would_change_it: Vec<String>,
    /// Risk summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<Risk>,
    /// Digest of the active policy.
    pub policy_digest: String,
    /// Digest of the calibration artefact, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibration_digest: Option<String>,
    /// Capability, when the action may execute under a mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<Capability>,
    /// Fallbacks taken while deciding.
    pub degradations: Vec<Degradation>,
    /// Time spent deciding, microseconds.
    pub latency_us: u64,
    /// Digest of this decision with `decision_digest` set to `""`.
    pub decision_digest: String,
}

impl GateDecision {
    /// Fill `decision_digest` from the rest of the decision.
    pub fn seal(&mut self) -> Result<(), canon::CanonError> {
        self.decision_digest = String::new();
        self.decision_digest = canon::digest(self)?;
        Ok(())
    }

    /// Check that `decision_digest` matches the content.
    pub fn verify_seal(&self) -> Result<bool, canon::CanonError> {
        let mut copy = self.clone();
        copy.decision_digest = String::new();
        Ok(canon::digest(&copy)? == self.decision_digest)
    }
}

/// Outcome bucket of an execution (gnomon's three buckets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    /// The tool ran and produced a result (success or ordinary failure).
    Result,
    /// The harness or tool refused.
    Refusal,
    /// The apparatus failed (timeout, crash, missing tool).
    ApparatusFailure,
}

/// What happened after a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecord {
    /// Always [`CONTRACT_VERSION`].
    pub contract_version: String,
    /// Harness session.
    pub session_id: String,
    /// Harness tool-call id (joins to the envelope).
    pub tool_use_id: String,
    /// Decision this execution followed, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_id: Option<String>,
    /// Whether the tool actually ran.
    pub executed: bool,
    /// Mode it ran in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ExecMode>,
    /// Exit code, for shell tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    /// Whether the harness reported an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Outcome bucket.
    pub bucket: Bucket,
    /// Duration, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// RFC 3339.
    pub ts: String,
}

/// Kinds of ledger entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerKind {
    /// An envelope (redacted).
    Envelope,
    /// A decision.
    Decision,
    /// A human decision.
    Human,
    /// An execution record.
    Execution,
    /// An outcome label.
    Outcome,
    /// A degradation.
    Degradation,
    /// A signed checkpoint.
    Checkpoint,
}

/// One hash-chained ledger line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedgerEntry {
    /// 0-based sequence within the session chain.
    pub seq: u64,
    /// Session this chain belongs to.
    pub session_id: String,
    /// Entry kind.
    pub kind: LedgerKind,
    /// Digest of `body`.
    pub body_digest: String,
    /// Body (may be omitted under digest-only recording).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    /// Hash of the previous entry, or the genesis value.
    pub prev_hash: String,
    /// `sha256(JCS(entry with hash = ""))`.
    pub hash: String,
    /// Daemon clock, RFC 3339.
    pub recorded_at: String,
}

/// Genesis `prev_hash` for every chain.
pub const GENESIS: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gt001_ladder_order_matches_rungs() {
        for (i, v) in Verdict::LADDER.iter().enumerate() {
            assert_eq!(usize::from(v.rung()), i);
        }
        assert!(Verdict::Block > Verdict::AskHuman);
        assert!(Verdict::Allow < Verdict::Sandbox);
    }

    #[test]
    fn test_gt002_only_first_three_rungs_execute() {
        let executing: Vec<_> = Verdict::LADDER.iter().filter(|v| v.executes()).collect();
        assert_eq!(
            executing,
            [&Verdict::Allow, &Verdict::Sandbox, &Verdict::Canary]
        );
    }

    #[test]
    fn test_unknown_fields_rejected() {
        let bad = r#"{"id":"x","detail":"y","extra":1}"#;
        assert!(serde_json::from_str::<Degradation>(bad).is_err());
    }
}
