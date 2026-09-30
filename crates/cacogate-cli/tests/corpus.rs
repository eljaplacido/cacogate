//! Rules against real agent traffic, not only hand-written fixtures.
//!
//! PO-002 proves every rule *can* fire. These prove what the rules *do* on
//! commands agents actually issued (harvested from local sessions, made
//! portable and labelled by `scripts/make_corpus_fixture.py`; see
//! `conformance/corpus/README.md`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/corpus")
        .join(name)
}

fn cacogate(data: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cacogate"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        // The fixture's paths were rewritten to this home.
        .env("HOME", "/home/dev")
        .env("CACOGATE_DATA_DIR", data)
        .env("CACOGATE_CONFIG", data.join("absent.toml"))
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn key(v: &Value) -> String {
    format!("{}|{}|{}", v["tool"], v["args"], v["cwd"])
}

fn lines(p: &Path) -> Vec<Value> {
    std::fs::read_to_string(p)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn test_po009_real_firings_still_fire_and_hazards_are_held() {
    let data = tempfile::tempdir().unwrap();
    let fixture = lines(&corpus("real_firings.jsonl"));
    assert!(fixture.len() >= 20, "fixture too small to mean anything");
    let out = data.path().join("fired.jsonl");
    let o = cacogate(
        data.path(),
        &[
            "corpus",
            corpus("real_firings.jsonl").to_str().unwrap(),
            "--fired-out",
            out.to_str().unwrap(),
        ],
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let got: HashMap<String, Value> = lines(&out).into_iter().map(|v| (key(&v), v)).collect();
    let mut hazards = 0;
    for row in &fixture {
        let label = row["label"].as_str().unwrap();
        assert!(
            matches!(label, "hazard" | "policy"),
            "unknown label {label}"
        );
        let g = got
            .get(&key(row))
            .unwrap_or_else(|| panic!("no rule fires any more on: {}", row["why"]));
        let sorted = |v: &Value| {
            let mut s: Vec<String> = v
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap().to_owned())
                .collect();
            s.sort();
            s
        };
        assert_eq!(
            sorted(&g["fired"]),
            sorted(&row["fired"]),
            "rules changed on: {}",
            row["why"]
        );
        if label == "hazard" {
            hazards += 1;
            let held = !matches!(g["verdict"].as_str(), Some("allow" | "sandbox" | "canary"));
            assert!(held, "hazard not held: {} ({})", row["why"], g["verdict"]);
        }
    }
    assert!(hazards >= 10);
}

#[test]
fn test_po010_nothing_fires_on_real_benign_traffic() {
    let data = tempfile::tempdir().unwrap();
    let benign = corpus("real_benign.jsonl");
    assert!(lines(&benign).len() >= 150);
    let o = cacogate(data.path(), &["corpus", benign.to_str().unwrap(), "--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let r: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(r["skipped"], 0);
    assert_eq!(
        r["by_rule"],
        serde_json::json!({}),
        "rules fired on ordinary agent work: {}",
        r["examples"]
    );
    // Held benign rows are fail-safe "could not measure" holds, never a rule
    // firing. Measured 2026-09-28: 2 of 180, both writes to `$S/…` after
    // `S=/tmp/…` in the same command (variable tracking is a listed follow-up).
    assert_eq!(r["would_hold"], r["held_unmeasured"]);
    assert!(r["held_unmeasured"].as_u64().unwrap() <= 3);
}

#[test]
fn test_po011_fixed_false_positives_stay_fixed_on_real_traffic() {
    // Real commands that fired a rule wrongly before 2026-09-28's corpus
    // replay (curl | python3 -c, systemctl --user, gh release download).
    let data = tempfile::tempdir().unwrap();
    let f = corpus("real_fixed_false_positives.jsonl");
    assert!(lines(&f).len() >= 5);
    let o = cacogate(data.path(), &["corpus", f.to_str().unwrap(), "--json"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let r: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        r["by_rule"],
        serde_json::json!({}),
        "a fixed false positive is back: {}",
        r["examples"]
    );
}
