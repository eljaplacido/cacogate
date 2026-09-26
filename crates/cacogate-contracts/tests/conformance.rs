//! Conformance: digest vectors, golden enumerations, and schema validation.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::missing_panics_doc
)]

use cacogate_contracts::{
    ActionClass, ActionEnvelope, Bucket, CONTRACT_VERSION, Check, CheckStatus, ExecutionRecord,
    GateDecision, Harness, HarnessName, Pillar, Tool, Verdict, canon,
};
use serde_json::{Value, json};
use std::path::PathBuf;

fn repo_file(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn load(rel: &str) -> Value {
    let text = std::fs::read_to_string(repo_file(rel)).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn test_cf001_digest_vectors_match() {
    let vectors = load("conformance/vectors/digest_vectors.json");
    let list = vectors["vectors"].as_array().unwrap();
    assert!(!list.is_empty());
    for v in list {
        let name = v["name"].as_str().unwrap();
        let canonical = canon::canonical_json(&v["input"]).unwrap();
        assert_eq!(
            canonical,
            v["canonical"].as_str().unwrap(),
            "canonical text for {name}"
        );
        assert_eq!(
            canon::digest(&v["input"]).unwrap(),
            v["digest"].as_str().unwrap(),
            "digest for {name}"
        );
    }
}

#[test]
fn test_cf002_golden_verdict_ladder_matches_code() {
    let golden = load("conformance/verdicts.golden.json");
    let ladder: Vec<String> = Verdict::LADDER
        .iter()
        .map(|v| v.as_str().to_owned())
        .collect();
    let expected: Vec<String> = serde_json::from_value(golden["verdict_ladder"].clone()).unwrap();
    assert_eq!(ladder, expected);
    assert_eq!(
        golden["expected_count"]["verdict_ladder"],
        json!(ladder.len())
    );
    for v in Verdict::LADDER {
        assert_eq!(
            golden["executes"][v.as_str()],
            json!(v.executes()),
            "executes({})",
            v.as_str()
        );
    }
}

#[test]
fn test_cf003_golden_fail_classes_match_code() {
    let golden = load("conformance/verdicts.golden.json");
    let table = golden["fail_class"].as_object().unwrap();
    assert_eq!(table.len(), ActionClass::ALL.len());
    for class in ActionClass::ALL {
        let want = table[class.as_str()].as_str().unwrap();
        assert_eq!(
            class.fail_verdict().as_str(),
            want,
            "fail class for {}",
            class.as_str()
        );
    }
}

#[test]
fn test_cf004_golden_ladder_vectors() {
    let vectors = load("conformance/vectors/ladder_vectors.json");
    for v in vectors["vectors"].as_array().unwrap() {
        let allows: Vec<Verdict> = serde_json::from_value(v["allows"].clone()).unwrap();
        let want: Verdict = serde_json::from_value(v["expect"].clone()).unwrap();
        assert_eq!(allows.into_iter().max().unwrap(), want);
    }
}

fn validator(schema_file: &str) -> jsonschema::Validator {
    let mut opts = jsonschema::options();
    for f in [
        "verdict",
        "action-envelope",
        "gate-decision",
        "execution-record",
        "ledger-entry",
    ] {
        let doc = load(&format!("contracts/v0/{f}.schema.json"));
        let uri = format!("https://cacogate.local/contracts/v0/{f}.schema.json");
        opts = opts.with_resource(uri, jsonschema::Resource::from_contents(doc).unwrap());
    }
    opts.build(&load(&format!("contracts/v0/{schema_file}.schema.json")))
        .unwrap()
}

fn assert_valid(schema: &str, value: &Value) {
    let v = validator(schema);
    let errors: Vec<String> = v.iter_errors(value).map(|e| e.to_string()).collect();
    assert!(errors.is_empty(), "{schema}: {errors:?}\n{value:#}");
}

pub fn sample_envelope() -> ActionEnvelope {
    let mut args = serde_json::Map::new();
    args.insert("command".into(), json!("git push origin main"));
    let action_digest =
        cacogate_contracts::action_digest("Bash", &args, "/repo", Some("/repo")).unwrap();
    ActionEnvelope {
        contract_version: CONTRACT_VERSION.into(),
        envelope_id: "env_1".into(),
        harness: Harness {
            name: HarnessName::ClaudeCode,
            version: "2.1.283".into(),
            adapter_version: "0.0.1".into(),
            interactive: Some(true),
        },
        session_id: "s1".into(),
        turn_id: None,
        tool_use_id: "toolu_1".into(),
        agent: None,
        model: None,
        tool: Tool {
            raw_name: "Bash".into(),
            kind: None,
        },
        args,
        cwd: "/repo".into(),
        repo: Some(cacogate_contracts::Repo {
            root: Some("/repo".into()),
            head: None,
            branch: Some("main".into()),
            dirty: None,
        }),
        surface_digest: None,
        ts: "2026-09-26T12:00:00Z".into(),
        action_digest,
    }
}

#[test]
fn test_cf005_rust_values_validate_against_schemas() {
    let env = sample_envelope();
    assert_valid("action-envelope", &serde_json::to_value(&env).unwrap());

    let mut decision = GateDecision {
        contract_version: CONTRACT_VERSION.into(),
        decision_id: "d_1".into(),
        envelope_digest: canon::digest(&env).unwrap(),
        verdict: Verdict::RequestEvidence,
        mode: Some(cacogate_contracts::GateMode::Shadow),
        checks: vec![Check {
            pillar: Pillar::Symbolic,
            id: "DEF-VCS-003".into(),
            status: CheckStatus::Failed,
            allows: Verdict::RequestEvidence,
            rule_class: Some(cacogate_contracts::RuleClass::Defeasible),
            detail: Some("no passing test run since last edit".into()),
            evidence_refs: vec![],
        }],
        obligations: vec![],
        constraints: vec![],
        rationale: None,
        what_would_change_it: vec![],
        risk: None,
        policy_digest: canon::digest(&json!({"p": 1})).unwrap(),
        calibration_digest: None,
        capability: None,
        degradations: vec![],
        latency_us: 42,
        decision_digest: String::new(),
    };
    decision.seal().unwrap();
    assert!(decision.verify_seal().unwrap());
    assert_valid("gate-decision", &serde_json::to_value(&decision).unwrap());

    let exec = ExecutionRecord {
        contract_version: CONTRACT_VERSION.into(),
        session_id: "s1".into(),
        tool_use_id: "toolu_1".into(),
        decision_id: Some("d_1".into()),
        executed: true,
        mode: None,
        exit_code: Some(0),
        is_error: Some(false),
        bucket: Bucket::Result,
        duration_ms: None,
        ts: "2026-09-26T12:00:01Z".into(),
    };
    assert_valid("execution-record", &serde_json::to_value(&exec).unwrap());
}

#[test]
fn test_cf006_schema_rejects_unknown_fields() {
    let mut v = serde_json::to_value(sample_envelope()).unwrap();
    v["surprise"] = json!(true);
    assert!(!validator("action-envelope").is_valid(&v));
}

#[test]
fn test_cf007_schema_enums_match_code() {
    let schema = load("contracts/v0/verdict.schema.json");
    let verdicts: Vec<Verdict> =
        serde_json::from_value(schema["$defs"]["Verdict"]["enum"].clone()).unwrap();
    assert_eq!(verdicts, Verdict::LADDER.to_vec());
    let classes: Vec<ActionClass> =
        serde_json::from_value(schema["$defs"]["ActionClass"]["enum"].clone()).unwrap();
    assert_eq!(classes.len(), ActionClass::ALL.len());
    let statuses: Vec<CheckStatus> =
        serde_json::from_value(schema["$defs"]["CheckStatus"]["enum"].clone()).unwrap();
    assert_eq!(statuses.len(), 4);
}
