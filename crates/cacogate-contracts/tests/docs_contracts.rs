//! Three-point rule, doc leg: every enum table in `docs/CONTRACTS.md` lists
//! exactly the schema's values, in the schema's order.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use serde_json::Value;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn schema(file: &str) -> Value {
    let t = std::fs::read_to_string(root().join("contracts/v0").join(file)).unwrap();
    serde_json::from_str(&t).unwrap()
}

fn schema_enum(file: &str, pointer: &str) -> Vec<String> {
    schema(file)
        .pointer(pointer)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{file}{pointer} is not an enum array"))
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

/// First-column backticked values of the table under the `name` heading.
fn doc_enum(doc: &str, name: &str) -> Vec<String> {
    let head = format!("### `{name}`");
    let start = doc
        .find(&head)
        .unwrap_or_else(|| panic!("docs/CONTRACTS.md has no section {head}"));
    let body = &doc[start + head.len()..];
    let end = body.find("\n#").unwrap_or(body.len());
    body[..end]
        .lines()
        .filter_map(|l| l.strip_prefix("| `"))
        .filter_map(|l| l.split_once('`').map(|(v, _)| v.to_owned()))
        .collect()
}

#[test]
fn test_cf009_contracts_doc_tables_match_schemas() {
    let doc = std::fs::read_to_string(root().join("docs/CONTRACTS.md")).unwrap();
    let cases = [
        ("Verdict", "verdict.schema.json", "/$defs/Verdict/enum"),
        (
            "CheckStatus",
            "verdict.schema.json",
            "/$defs/CheckStatus/enum",
        ),
        ("Pillar", "verdict.schema.json", "/$defs/Pillar/enum"),
        ("RuleClass", "verdict.schema.json", "/$defs/RuleClass/enum"),
        (
            "ActionClass",
            "verdict.schema.json",
            "/$defs/ActionClass/enum",
        ),
        (
            "HarnessName",
            "action-envelope.schema.json",
            "/properties/harness/properties/name/enum",
        ),
        (
            "ToolKind",
            "action-envelope.schema.json",
            "/properties/tool/properties/kind/enum",
        ),
        (
            "GateMode",
            "gate-decision.schema.json",
            "/properties/mode/enum",
        ),
        (
            "ExecMode",
            "execution-record.schema.json",
            "/properties/mode/enum",
        ),
        (
            "Bucket",
            "execution-record.schema.json",
            "/properties/bucket/enum",
        ),
        (
            "ConformalStatus",
            "gate-decision.schema.json",
            "/properties/risk/properties/conformal/properties/status/enum",
        ),
        (
            "Identification",
            "gate-decision.schema.json",
            "/properties/risk/properties/causal/properties/identification/enum",
        ),
        (
            "ObligationKind",
            "gate-decision.schema.json",
            "/properties/obligations/items/properties/kind/enum",
        ),
        (
            "LedgerKind",
            "ledger-entry.schema.json",
            "/properties/kind/enum",
        ),
    ];
    for (name, file, pointer) in cases {
        assert_eq!(
            doc_enum(&doc, name),
            schema_enum(file, pointer),
            "docs/CONTRACTS.md `{name}` != {file}{pointer}"
        );
    }
}

#[test]
fn test_cf010_fail_class_table_matches_golden() {
    let doc = std::fs::read_to_string(root().join("docs/CONTRACTS.md")).unwrap();
    let golden: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("conformance/verdicts.golden.json")).unwrap(),
    )
    .unwrap();
    let start = doc.find("### `ActionClass`").unwrap();
    let section = &doc[start..];
    let section = &section[..section[4..].find("\n###").map_or(section.len(), |i| i + 4)];
    for (class, verdict) in golden["fail_class"].as_object().unwrap() {
        let row = format!("| `{class}` | `{}` |", verdict.as_str().unwrap());
        assert!(section.contains(&row), "missing or wrong row: {row}");
    }
}
