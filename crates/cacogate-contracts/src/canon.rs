//! RFC 8785 JSON Canonicalization Scheme (JCS) and `sha256:` digests.
//!
//! Every digest in cacogate (action, envelope, decision, ledger hash) is
//! `sha256(JCS(value))`, so Rust, Python and TypeScript produce identical
//! bytes. `conformance/vectors/digest_vectors.json` pins this across languages.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;

/// Errors raised while canonicalising.
#[derive(Debug, thiserror::Error)]
pub enum CanonError {
    /// The value could not be converted to JSON.
    #[error("serialise: {0}")]
    Serialise(#[from] serde_json::Error),
    /// JCS forbids NaN and infinities.
    #[error("non-finite number is not representable in JCS")]
    NonFinite,
}

/// Canonical JSON text of any serialisable value.
pub fn canonical_json<T: Serialize + ?Sized>(value: &T) -> Result<String, CanonError> {
    let v = serde_json::to_value(value)?;
    let mut out = String::new();
    write_value(&v, &mut out)?;
    Ok(out)
}

/// `sha256:<hex>` over the canonical JSON of `value`.
pub fn digest<T: Serialize + ?Sized>(value: &T) -> Result<String, CanonError> {
    Ok(digest_bytes(canonical_json(value)?.as_bytes()))
}

/// `sha256:<hex>` over raw bytes.
pub fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn write_value(v: &Value, out: &mut String) -> Result<(), CanonError> {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(n, out)?,
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            // JCS orders members by the UTF-16 code units of their names.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                if let Some(child) = map.get(*key) {
                    write_value(child, out)?;
                }
            }
            out.push('}');
        }
    }
    Ok(())
}

fn write_number(n: &serde_json::Number, out: &mut String) -> Result<(), CanonError> {
    if let Some(i) = n.as_i64() {
        let _ = write!(out, "{i}");
        return Ok(());
    }
    if let Some(u) = n.as_u64() {
        let _ = write!(out, "{u}");
        return Ok(());
    }
    let f = n.as_f64().ok_or(CanonError::NonFinite)?;
    if !f.is_finite() {
        return Err(CanonError::NonFinite);
    }
    // ECMAScript Number.prototype.toString, as JCS requires.
    let mut buf = ryu_js::Buffer::new();
    out.push_str(buf.format_finite(f));
    Ok(())
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_jcs_orders_keys_and_strips_whitespace() {
        let v = json!({"b": 1, "a": [true, null, "x"], "c": {"z": 1, "y": 2}});
        assert_eq!(
            canonical_json(&v).unwrap(),
            r#"{"a":[true,null,"x"],"b":1,"c":{"y":2,"z":1}}"#
        );
    }

    #[test]
    fn test_jcs_numbers_follow_ecmascript() {
        let v = json!([1.0, 0.05, 1e21, 1e-7, -0.0, 100]);
        assert_eq!(canonical_json(&v).unwrap(), "[1,0.05,1e+21,1e-7,0,100]");
    }

    #[test]
    fn test_jcs_escapes_control_characters_lowercase() {
        let v = json!("a\u{1f}b\n\"\\");
        assert_eq!(canonical_json(&v).unwrap(), r#""a\u001fb\n\"\\""#);
    }

    #[test]
    fn test_digest_prefix_and_length() {
        let d = digest(&json!({})).unwrap();
        assert!(d.starts_with("sha256:"));
        assert_eq!(d.len(), 7 + 64);
    }

    proptest::proptest! {
        #[test]
        fn test_digest_is_insensitive_to_key_order(a in 0i64..1000, b in "[a-z]{0,8}") {
            let one = json!({"x": a, "y": b.clone()});
            let two: Value = serde_json::from_str(&format!(r#"{{"y":{},"x":{a}}}"#, serde_json::to_string(&b).unwrap())).unwrap();
            proptest::prop_assert_eq!(digest(&one).unwrap(), digest(&two).unwrap());
        }
    }
}
