//! Redaction of envelope arguments before they reach the ledger.
//!
//! File bodies and edit payloads are replaced by their length and digest:
//! the ledger can prove what was written without storing it. Commands are
//! kept verbatim because a reviewer needs them.

use serde_json::{Map, Value, json};

const BODY_KEYS: [&str; 12] = [
    "content",
    "new_string",
    "old_string",
    "newString",
    "oldString",
    "new_source",
    "patch",
    "patchText",
    "input",
    "edits",
    "text",
    "diff",
];

/// Longest string kept verbatim.
pub const MAX_INLINE: usize = 4096;

/// Redacted copy of `args`.
pub fn redact_args(args: &Map<String, Value>) -> Map<String, Value> {
    args.iter()
        .map(|(k, v)| {
            let redact =
                BODY_KEYS.contains(&k.as_str()) || v.as_str().is_some_and(|s| s.len() > MAX_INLINE);
            let v = if redact { summary(v) } else { v.clone() };
            (k.clone(), v)
        })
        .collect()
}

fn summary(v: &Value) -> Value {
    let text = match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    json!({
        "redacted": true,
        "len": text.len(),
        "digest": cacogate_contracts::canon::digest_bytes(text.as_bytes()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rd001_bodies_are_redacted_commands_kept() {
        let mut m = Map::new();
        m.insert("command".into(), json!("git push"));
        m.insert("content".into(), json!("SECRET=1"));
        let r = redact_args(&m);
        assert_eq!(r["command"], json!("git push"));
        assert_eq!(r["content"]["redacted"], json!(true));
        assert_eq!(r["content"]["len"], json!(8));
    }
}
