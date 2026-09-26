//! Engine: decide, record, and track session state. Synchronous and
//! transport-free so the HTTP server, tests and the CLI can share it.

use cacogate_contracts::{
    ActionEnvelope, Degradation, ExecutionRecord, GateDecision, GateMode, LedgerKind, canon,
};
use cacogate_harness::CheckResponse;
use cacogate_kernel::{
    CompiledPolicy, DecideInput, KernelEnv, SessionState, decide, extract, packet, redact,
};
use cacogate_ledger::{IndexRow, Ledger, LedgerError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Engine errors.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// Ledger failure.
    #[error("ledger: {0}")]
    Ledger(#[from] LedgerError),
    /// Canonicalisation failure.
    #[error("canon: {0}")]
    Canon(#[from] canon::CanonError),
    /// The envelope's action digest does not match its contents.
    #[error("action_digest does not match the envelope contents")]
    DigestMismatch,
    /// Unsupported contract version.
    #[error("unsupported contract_version {0}")]
    Version(String),
    /// Internal lock poisoned.
    #[error("engine lock poisoned")]
    Poisoned,
}

/// What the engine remembers about a decided-but-not-yet-executed call.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pending {
    decision_id: String,
    is_edit: bool,
    is_test: bool,
}

#[derive(Debug, Default)]
struct Track {
    state: SessionState,
    pending: HashMap<String, Pending>,
}

/// Summary counters for `status`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    /// Gate mode.
    pub mode: String,
    /// Policy name.
    pub policy: String,
    /// Policy digest.
    pub policy_digest: String,
    /// Decisions by verdict.
    pub by_verdict: BTreeMap<String, u64>,
    /// Decisions by class.
    pub by_class: BTreeMap<String, u64>,
    /// Decisions by harness.
    pub by_harness: BTreeMap<String, u64>,
    /// Would-have-held count (verdict does not execute).
    pub would_hold: u64,
    /// Total decisions.
    pub total: u64,
    /// Executions joined to a decision since start.
    pub executions_joined: u64,
    /// Median decision latency in microseconds since start.
    pub p50_latency_us: u64,
    /// 95th percentile decision latency since start.
    pub p95_latency_us: u64,
}

/// The decision engine.
#[derive(Debug)]
pub struct Engine {
    ledger: Ledger,
    policy: CompiledPolicy,
    kenv: KernelEnv,
    mode: GateMode,
    tracks: Mutex<HashMap<String, Track>>,
    counter: AtomicU64,
    latencies: Mutex<Vec<u64>>,
    joined: AtomicU64,
    degraded_policy: Option<Degradation>,
}

/// RFC 3339 timestamp, UTC.
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

fn mode_str(m: GateMode) -> &'static str {
    match m {
        GateMode::Shadow => "shadow",
        GateMode::Enforce => "enforce",
    }
}

impl Engine {
    /// Build an engine. An invalid configured policy falls back to
    /// `builtin:strict` with a recorded degradation, never to allow-all.
    pub fn new(
        ledger: Ledger,
        policy: &cacogate_kernel::Policy,
        home: String,
        mode: GateMode,
    ) -> Result<Engine, cacogate_kernel::policy::PolicyError> {
        let (compiled, degraded_policy) =
            match cacogate_kernel::policy::validate(policy).and_then(|()| policy.compile()) {
                Ok(c) => (c, None),
                Err(e) => (
                    cacogate_kernel::Policy::builtin_strict().compile()?,
                    Some(Degradation {
                        id: "policy_invalid".into(),
                        detail: e.to_string(),
                    }),
                ),
            };
        Ok(Engine {
            ledger,
            policy: compiled,
            kenv: KernelEnv { home },
            mode,
            tracks: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
            latencies: Mutex::new(Vec::new()),
            joined: AtomicU64::new(0),
            degraded_policy,
        })
    }

    /// Gate mode.
    pub fn mode(&self) -> GateMode {
        self.mode
    }

    /// Ledger.
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// Active policy.
    pub fn policy(&self) -> &CompiledPolicy {
        &self.policy
    }

    fn next_id(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let ms = time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
        format!("d_{ms:x}{:04x}", n & 0xffff)
    }

    /// Rebuild a session's state from its ledger chain (after a restart).
    fn replay(&self, session: &str) -> Track {
        let mut track = Track::default();
        let Ok(entries) = self.ledger.read_session(session) else {
            return track;
        };
        for e in entries {
            let Some(body) = e.body else { continue };
            match e.kind {
                LedgerKind::Decision => {
                    let tu = body
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let p = serde_json::from_value::<Pending>(
                        body.get("pending").cloned().unwrap_or(Value::Null),
                    );
                    if let (Some(tu), Ok(p)) = (tu, p) {
                        track.state.decisions += 1;
                        track.pending.insert(tu, p);
                    }
                }
                LedgerKind::Execution => {
                    if let Ok(x) = serde_json::from_value::<ExecutionRecord>(body) {
                        apply_execution(&mut track, &x);
                    }
                }
                _ => {}
            }
        }
        track
    }

    fn with_track<R>(
        &self,
        session: &str,
        f: impl FnOnce(&mut Track) -> R,
    ) -> Result<R, EngineError> {
        let mut tracks = self.tracks.lock().map_err(|_| EngineError::Poisoned)?;
        if !tracks.contains_key(session) {
            let t = self.replay(session);
            tracks.insert(session.to_owned(), t);
        }
        let track = tracks.entry(session.to_owned()).or_default();
        Ok(f(track))
    }

    /// Decide on an envelope and record envelope + decision in the ledger.
    pub fn check(&self, envelope: &ActionEnvelope) -> Result<CheckResponse, EngineError> {
        let started = Instant::now();
        if envelope.contract_version != cacogate_contracts::CONTRACT_VERSION {
            return Err(EngineError::Version(envelope.contract_version.clone()));
        }
        if envelope.recompute_action_digest()? != envelope.action_digest {
            return Err(EngineError::DigestMismatch);
        }
        let envelope_digest = canon::digest(envelope)?;
        let facts = extract(envelope, &self.kenv);
        let session = self.with_track(&envelope.session_id, |t| t.state.clone())?;
        let mut decision = decide(&DecideInput {
            envelope,
            facts: &facts,
            session: &session,
            policy: &self.policy,
            mode: self.mode,
            decision_id: self.next_id(),
            envelope_digest,
        });
        if let Some(d) = &self.degraded_policy {
            decision.degradations.push(d.clone());
        }
        decision.latency_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        decision.seal()?;
        let packet = packet::render(&decision, &facts);
        self.record(envelope, &decision, &facts)?;
        if let Ok(mut l) = self.latencies.lock() {
            l.push(decision.latency_us);
        }
        Ok(CheckResponse { decision, packet })
    }

    fn record(
        &self,
        envelope: &ActionEnvelope,
        decision: &GateDecision,
        facts: &cacogate_kernel::ActionFacts,
    ) -> Result<(), EngineError> {
        let now = now_rfc3339();
        let mut redacted = envelope.clone();
        redacted.args = redact::redact_args(&envelope.args);
        let session = &envelope.session_id;
        let env_body = serde_json::to_value(&redacted).map_err(LedgerError::from)?;
        self.ledger
            .append(session, LedgerKind::Envelope, env_body, &now)?;
        let pending = Pending {
            decision_id: decision.decision_id.clone(),
            is_edit: facts.is_edit(),
            is_test: facts.is_test_run(),
        };
        let body = json!({
            "tool_use_id": envelope.tool_use_id,
            "decision": decision,
            "facts": {
                "class": facts.class,
                "tool_kind": facts.tool_kind,
                "summary": facts.summary,
                "paths_written": facts.paths_written,
                "parse_status": facts.parse_status,
                "reversibility": facts.reversibility,
            },
            "pending": pending,
        });
        let entry = self
            .ledger
            .append(session, LedgerKind::Decision, body, &now)?;
        let harness = serde_json::to_value(envelope.harness.name)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        self.ledger.index(&IndexRow {
            decision_id: decision.decision_id.clone(),
            session_id: session.clone(),
            seq: entry.seq,
            recorded_at: now,
            verdict: decision.verdict.as_str().into(),
            mode: mode_str(self.mode).into(),
            class: facts.class.as_str().into(),
            harness,
            tool: envelope.tool.raw_name.clone(),
            summary: facts.summary.clone(),
            tool_use_id: envelope.tool_use_id.clone(),
        })?;
        self.with_track(session, |t| {
            t.state.decisions += 1;
            t.pending.insert(envelope.tool_use_id.clone(), pending);
        })?;
        Ok(())
    }

    /// Record an execution and update session state.
    pub fn execution(&self, mut rec: ExecutionRecord) -> Result<ExecutionRecord, EngineError> {
        let decision_id = self.with_track(&rec.session_id, |t| {
            let id = t
                .pending
                .get(&rec.tool_use_id)
                .map(|p| p.decision_id.clone());
            apply_execution(t, &rec);
            id
        })?;
        if rec.decision_id.is_none() {
            rec.decision_id = decision_id;
        }
        if rec.decision_id.is_some() {
            self.joined.fetch_add(1, Ordering::Relaxed);
        }
        let body = serde_json::to_value(&rec).map_err(LedgerError::from)?;
        self.ledger
            .append(&rec.session_id, LedgerKind::Execution, body, &now_rfc3339())?;
        Ok(rec)
    }

    /// Session state snapshot.
    pub fn session_state(&self, session: &str) -> Result<SessionState, EngineError> {
        self.with_track(session, |t| t.state.clone())
    }

    /// Counters over the decision index.
    pub fn status(&self) -> Result<Status, EngineError> {
        let rows = self.ledger.read_index(0)?;
        let mut s = Status {
            mode: mode_str(self.mode).into(),
            policy: self.policy.policy.name.clone(),
            policy_digest: self.policy.digest.clone(),
            total: rows.len() as u64,
            executions_joined: self.joined.load(Ordering::Relaxed),
            ..Status::default()
        };
        for r in &rows {
            *s.by_verdict.entry(r.verdict.clone()).or_default() += 1;
            *s.by_class.entry(r.class.clone()).or_default() += 1;
            *s.by_harness.entry(r.harness.clone()).or_default() += 1;
            if !matches!(r.verdict.as_str(), "allow" | "sandbox" | "canary") {
                s.would_hold += 1;
            }
        }
        if let Ok(l) = self.latencies.lock() {
            let mut v = l.clone();
            v.sort_unstable();
            s.p50_latency_us = percentile(&v, 50);
            s.p95_latency_us = percentile(&v, 95);
        }
        Ok(s)
    }
}

/// Nearest-rank percentile over sorted values.
fn percentile(sorted: &[u64], p: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p * sorted.len()).div_ceil(100).max(1);
    sorted.get(rank - 1).copied().unwrap_or(0)
}

fn apply_execution(track: &mut Track, rec: &ExecutionRecord) {
    let pending = track.pending.remove(&rec.tool_use_id);
    let ok = rec.executed && rec.is_error != Some(true) && rec.exit_code.is_none_or(|c| c == 0);
    match pending {
        Some(p) if p.is_test && ok => track.state.record_test_pass(),
        Some(p) if p.is_edit && rec.executed => track.state.record_edit(),
        _ => track.state.record_other(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cacogate_contracts::{Bucket, CONTRACT_VERSION, Harness, HarnessName, Repo, Tool, Verdict};

    fn engine(dir: &std::path::Path) -> Engine {
        Engine::new(
            Ledger::open(dir).unwrap(),
            &cacogate_kernel::Policy::builtin_strict(),
            "/home/u".into(),
            GateMode::Shadow,
        )
        .unwrap()
    }

    fn env(session: &str, tu: &str, tool: &str, args: &Value) -> ActionEnvelope {
        let args = args.as_object().cloned().unwrap();
        ActionEnvelope {
            contract_version: CONTRACT_VERSION.into(),
            envelope_id: format!("e{tu}"),
            harness: Harness {
                name: HarnessName::ClaudeCode,
                version: "t".into(),
                adapter_version: "t".into(),
                interactive: None,
            },
            session_id: session.into(),
            turn_id: None,
            tool_use_id: tu.into(),
            agent: None,
            model: None,
            tool: Tool {
                raw_name: tool.into(),
                kind: None,
            },
            action_digest: cacogate_contracts::action_digest(tool, &args, "/repo", Some("/repo"))
                .unwrap(),
            args,
            cwd: "/repo".into(),
            repo: Some(Repo {
                root: Some("/repo".into()),
                head: None,
                branch: Some("feat/x".into()),
                dirty: None,
            }),
            surface_digest: None,
            ts: "t".into(),
        }
    }

    fn exec(session: &str, tu: &str, ok: bool) -> ExecutionRecord {
        ExecutionRecord {
            contract_version: CONTRACT_VERSION.into(),
            session_id: session.into(),
            tool_use_id: tu.into(),
            decision_id: None,
            executed: true,
            mode: None,
            exit_code: None,
            is_error: Some(!ok),
            bucket: Bucket::Result,
            duration_ms: None,
            ts: "t".into(),
        }
    }

    fn bash(session: &str, tu: &str, cmd: &str) -> ActionEnvelope {
        env(session, tu, "Bash", &json!({ "command": cmd }))
    }

    #[test]
    fn test_en001_edit_then_push_requests_evidence_until_tests_pass() {
        let d = tempfile::tempdir().unwrap();
        let e = engine(d.path());
        let edit = env(
            "s",
            "1",
            "Edit",
            &json!({"file_path": "/repo/a.rs", "old_string": "a", "new_string": "b"}),
        );
        assert_eq!(e.check(&edit).unwrap().decision.verdict, Verdict::Allow);
        e.execution(exec("s", "1", true)).unwrap();
        let r = e.check(&bash("s", "2", "git push origin feat/x")).unwrap();
        assert_eq!(r.decision.verdict, Verdict::RequestEvidence);
        assert!(r.packet.contains("REQUEST EVIDENCE"));
        // a failing test run does not count as evidence
        e.check(&bash("s", "3", "cargo test")).unwrap();
        e.execution(exec("s", "3", false)).unwrap();
        let r = e.check(&bash("s", "4", "git push origin feat/x")).unwrap();
        assert_eq!(r.decision.verdict, Verdict::RequestEvidence);
        e.check(&bash("s", "5", "cargo test")).unwrap();
        e.execution(exec("s", "5", true)).unwrap();
        let r = e.check(&bash("s", "6", "git push origin feat/x")).unwrap();
        assert_eq!(r.decision.verdict, Verdict::Allow);
    }

    #[test]
    fn test_en002_state_survives_restart_via_ledger_replay() {
        let d = tempfile::tempdir().unwrap();
        {
            let e = engine(d.path());
            e.check(&env(
                "s",
                "1",
                "Write",
                &json!({"file_path": "/repo/x", "content": "y"}),
            ))
            .unwrap();
            e.execution(exec("s", "1", true)).unwrap();
        }
        let e2 = engine(d.path());
        let r = e2.check(&bash("s", "2", "git push")).unwrap();
        assert_eq!(r.decision.verdict, Verdict::RequestEvidence);
        assert!(e2.ledger().verify_session("s").ok());
    }

    #[test]
    fn test_en003_tampered_digest_is_rejected() {
        let d = tempfile::tempdir().unwrap();
        let e = engine(d.path());
        let mut x = bash("s", "1", "ls");
        x.args.insert("command".into(), json!("rm -rf /"));
        assert!(matches!(e.check(&x), Err(EngineError::DigestMismatch)));
    }

    #[test]
    fn test_en004_ledger_stores_redacted_bodies() {
        let d = tempfile::tempdir().unwrap();
        let e = engine(d.path());
        let w = env(
            "s",
            "1",
            "Write",
            &json!({"file_path": "/repo/.env.local", "content": "API_KEY=supersecret"}),
        );
        e.check(&w).unwrap();
        let text = std::fs::read_to_string(e.ledger().session_path("s")).unwrap();
        assert!(!text.contains("supersecret"));
    }

    #[test]
    fn test_en005_decisions_are_sealed_and_indexed() {
        let d = tempfile::tempdir().unwrap();
        let e = engine(d.path());
        let r = e
            .check(&bash("s", "1", "git push --force origin main"))
            .unwrap();
        assert!(r.decision.verify_seal().unwrap());
        let s = e.status().unwrap();
        assert_eq!(s.total, 1);
        assert_eq!(s.would_hold, 1);
        assert_eq!(s.by_verdict.get("block"), Some(&1));
    }

    #[test]
    fn test_en006_percentile_nearest_rank() {
        assert_eq!(percentile(&[], 95), 0);
        assert_eq!(percentile(&[5], 50), 5);
        let v: Vec<u64> = (1..=100).collect();
        assert_eq!(percentile(&v, 50), 50);
        assert_eq!(percentile(&v, 95), 95);
    }
}
