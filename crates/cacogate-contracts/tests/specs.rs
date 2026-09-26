//! Article 1 guard: every test ID maps to an EARS requirement in `specs/`,
//! and every requirement there is pinned by at least one test.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Files under `dir` (recursively) whose extension is `ext`, skipping build output.
fn files(dir: &Path, ext: &str, recursive: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if recursive && name != "target" && name != "node_modules" && !name.starts_with('.') {
                files(&path, ext, recursive, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
}

fn id(prefix: &str, digits: &str) -> String {
    format!("{}-{digits}", prefix.to_ascii_uppercase())
}

/// Rust: `fn test_` + `[a-z]+` + exactly three digits + `_`.
fn rust_ids(text: &str) -> Vec<String> {
    const NEEDLE: &str = "fn test_";
    let mut ids = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find(NEEDLE) {
        rest = &rest[pos + NEEDLE.len()..];
        let letters = rest.bytes().take_while(u8::is_ascii_lowercase).count();
        let tail = &rest[letters..];
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        if letters > 0 && digits == 3 && tail.as_bytes().get(3) == Some(&b'_') {
            ids.push(id(&rest[..letters], &tail[..3]));
        }
    }
    ids
}

/// TypeScript: `"` + `[A-Z]{2}` + three digits + space (vitest describe names).
fn ts_ids(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut ids = Vec::new();
    for (i, _) in text.match_indices('"') {
        let Some(w) = b.get(i + 1..i + 7) else {
            continue;
        };
        if w[..2].iter().all(u8::is_ascii_uppercase)
            && w[2..5].iter().all(u8::is_ascii_digit)
            && w[5] == b' '
        {
            ids.push(id(&text[i + 1..i + 3], &text[i + 3..i + 6]));
        }
    }
    ids
}

/// Specs: `**AA-NNN**`.
fn spec_ids(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut ids = Vec::new();
    for (i, _) in text.match_indices("**") {
        let Some(w) = b.get(i + 2..i + 10) else {
            continue;
        };
        if w[..2].iter().all(u8::is_ascii_uppercase)
            && w[2] == b'-'
            && w[3..6].iter().all(u8::is_ascii_digit)
            && &w[6..8] == b"**"
        {
            ids.push(text[i + 2..i + 8].to_owned());
        }
    }
    ids
}

fn collect(
    paths: &[PathBuf],
    scan: fn(&str) -> Vec<String>,
    into: &mut BTreeMap<String, Vec<String>>,
) {
    let base = root();
    for p in paths {
        let text = std::fs::read_to_string(p).unwrap();
        let shown = p
            .strip_prefix(&base)
            .unwrap_or(p)
            .to_string_lossy()
            .into_owned();
        for i in scan(&text) {
            into.entry(i).or_default().push(shown.clone());
        }
    }
}

#[test]
fn test_cf008_every_test_id_has_a_requirement_and_back() {
    let base = root();

    let mut rs = Vec::new();
    files(&base.join("crates"), "rs", true, &mut rs);
    let mut ts = Vec::new();
    files(&base.join("adapters/test"), "ts", false, &mut ts);
    let mut md = Vec::new();
    files(&base.join("specs"), "md", false, &mut md);
    assert!(
        !rs.is_empty() && !md.is_empty(),
        "nothing to scan under {base:?}"
    );

    let mut tests = BTreeMap::new();
    collect(&rs, rust_ids, &mut tests);
    collect(&ts, ts_ids, &mut tests);
    let mut specs = BTreeMap::new();
    collect(&md, spec_ids, &mut specs);
    assert!(!tests.is_empty() && !specs.is_empty());

    let duplicated: Vec<_> = specs.iter().filter(|(_, at)| at.len() > 1).collect();
    assert!(
        duplicated.is_empty(),
        "requirement IDs defined more than once: {duplicated:?}"
    );

    let test_ids: BTreeSet<&String> = tests.keys().collect();
    let spec_ids: BTreeSet<&String> = specs.keys().collect();
    let unspecified: Vec<_> = test_ids
        .difference(&spec_ids)
        .map(|i| (i, &tests[*i]))
        .collect();
    let untested: Vec<_> = spec_ids
        .difference(&test_ids)
        .map(|i| (i, &specs[*i]))
        .collect();
    assert!(
        unspecified.is_empty(),
        "test IDs with no requirement in specs/: {unspecified:?}"
    );
    assert!(
        untested.is_empty(),
        "requirements in specs/ with no test: {untested:?}"
    );
}

#[test]
fn test_cf008_scanners_match_the_documented_forms() {
    // Built with concat! so this file's own source does not look like a test ID.
    assert_eq!(
        rust_ids(concat!(
            "fn te",
            "st_sh007_x() {} ",
            "fn te",
            "st_ab12_x fn te",
            "st_ab1234_x fn te",
            "st_unknown"
        )),
        vec!["SH-007"]
    );
    assert_eq!(
        ts_ids(r#"describe("AD001 x"); describe("AD0012 x"); "ab001 ""#),
        vec!["AD-001"]
    );
    assert_eq!(
        spec_ids("- **SH-007** — x. **Ab-001** **AB-01** **AB-0012**"),
        vec!["SH-007"]
    );
}
