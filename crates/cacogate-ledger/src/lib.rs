//! Append-only, hash-chained evidence ledger.
//!
//! One JSONL file per session under `<dir>/sessions/`. Each line is a
//! [`LedgerEntry`] whose `hash` is `sha256(JCS(entry with hash = ""))` and
//! whose `prev_hash` is the previous line's hash ([`GENESIS`] for the first).
//! Editing, deleting or reordering any line breaks verification from that
//! point on. The chain makes tampering detectable, not impossible: anchoring
//! heads elsewhere (checkpoints) is what makes truncation detectable too.
//!
//! `<dir>/index.jsonl` is a derived, unchained lookup table for decisions.

use cacogate_contracts::{GENESIS, LedgerEntry, LedgerKind, canon};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Ledger errors.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// Filesystem error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// Serialisation error.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Canonicalisation error.
    #[error("canon: {0}")]
    Canon(#[from] canon::CanonError),
    /// The existing chain is broken; refusing to append to it.
    #[error("chain for session {session} is broken at seq {seq}: {reason}")]
    Broken {
        /// Session.
        session: String,
        /// First bad sequence number.
        seq: u64,
        /// Why.
        reason: String,
    },
    /// Internal lock poisoned.
    #[error("ledger lock poisoned")]
    Poisoned,
}

/// A row of the decision index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRow {
    /// Decision id.
    pub decision_id: String,
    /// Session.
    pub session_id: String,
    /// Ledger sequence of the decision entry.
    pub seq: u64,
    /// When recorded.
    pub recorded_at: String,
    /// Verdict.
    pub verdict: String,
    /// Mode (shadow/enforce).
    pub mode: String,
    /// Action class.
    pub class: String,
    /// Harness.
    pub harness: String,
    /// Tool name.
    pub tool: String,
    /// One-line summary.
    pub summary: String,
    /// Tool-call id (joins to executions).
    pub tool_use_id: String,
}

/// Result of verifying one chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifyReport {
    /// Session.
    pub session_id: String,
    /// Entries read.
    pub entries: u64,
    /// Head hash, if any.
    pub head: Option<String>,
    /// First broken sequence number.
    pub broken_at: Option<u64>,
    /// Why it broke.
    pub reason: Option<String>,
}

impl VerifyReport {
    /// Whether the chain verified.
    pub fn ok(&self) -> bool {
        self.broken_at.is_none()
    }
}

/// Hash of an entry: its canonical form with `hash` set to "".
pub fn entry_hash(entry: &LedgerEntry) -> Result<String, canon::CanonError> {
    let mut e = entry.clone();
    e.hash = String::new();
    canon::digest(&e)
}

/// Verify a sequence of entries in memory. Pure.
pub fn verify_entries(session: &str, entries: &[LedgerEntry]) -> VerifyReport {
    let mut prev = GENESIS.to_owned();
    let mut report = VerifyReport {
        session_id: session.to_owned(),
        entries: entries.len() as u64,
        head: None,
        broken_at: None,
        reason: None,
    };
    for (i, e) in entries.iter().enumerate() {
        let i = i as u64;
        let fail = |why: String| (i, why);
        let problem = if e.seq != i {
            Some(fail(format!("expected seq {i}, found {}", e.seq)))
        } else if e.session_id != session {
            Some(fail(format!("entry belongs to session {}", e.session_id)))
        } else if e.prev_hash != prev {
            Some(fail("prev_hash does not match the previous entry".into()))
        } else if let Some(body) = &e.body {
            match canon::digest(body) {
                Ok(d) if d == e.body_digest => None,
                Ok(_) => Some(fail("body does not match body_digest".into())),
                Err(err) => Some(fail(format!("body not canonicalisable: {err}"))),
            }
        } else {
            None
        };
        let problem = problem.or_else(|| match entry_hash(e) {
            Ok(h) if h == e.hash => None,
            Ok(_) => Some(fail("hash does not match content".into())),
            Err(err) => Some(fail(format!("entry not canonicalisable: {err}"))),
        });
        if let Some((seq, why)) = problem {
            report.broken_at = Some(seq);
            report.reason = Some(why);
            return report;
        }
        prev.clone_from(&e.hash);
        report.head = Some(e.hash.clone());
    }
    report
}

/// File-system ledger. Safe to share between threads.
#[derive(Debug)]
pub struct Ledger {
    dir: PathBuf,
    heads: Mutex<HashMap<String, (u64, String)>>,
}

impl Ledger {
    /// Open (and create) a ledger directory.
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, LedgerError> {
        let dir = dir.into();
        fs::create_dir_all(dir.join("sessions"))?;
        Ok(Ledger {
            dir,
            heads: Mutex::new(HashMap::new()),
        })
    }

    /// Ledger directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Path of a session's chain file.
    pub fn session_path(&self, session: &str) -> PathBuf {
        self.dir
            .join("sessions")
            .join(format!("{}.jsonl", file_stem(session)))
    }

    /// Append a body to a session chain. `recorded_at` comes from the caller's clock.
    pub fn append(
        &self,
        session: &str,
        kind: LedgerKind,
        body: Value,
        recorded_at: &str,
    ) -> Result<LedgerEntry, LedgerError> {
        let mut heads = self.heads.lock().map_err(|_| LedgerError::Poisoned)?;
        let (next_seq, prev) = if let Some((seq, hash)) = heads.get(session) {
            (seq + 1, hash.clone())
        } else {
            let existing = self.read_session(session)?;
            let report = verify_entries(session, &existing);
            if let (Some(seq), Some(reason)) = (report.broken_at, report.reason.clone()) {
                return Err(LedgerError::Broken {
                    session: session.to_owned(),
                    seq,
                    reason,
                });
            }
            match existing.last() {
                Some(last) => (last.seq + 1, last.hash.clone()),
                None => (0, GENESIS.to_owned()),
            }
        };
        let mut entry = LedgerEntry {
            seq: next_seq,
            session_id: session.to_owned(),
            kind,
            body_digest: canon::digest(&body)?,
            body: Some(body),
            prev_hash: prev,
            hash: String::new(),
            recorded_at: recorded_at.to_owned(),
        };
        entry.hash = entry_hash(&entry)?;
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.session_path(session))?;
        f.write_all(line.as_bytes())?;
        f.flush()?;
        heads.insert(session.to_owned(), (entry.seq, entry.hash.clone()));
        Ok(entry)
    }

    /// Append a decision index row.
    pub fn index(&self, row: &IndexRow) -> Result<(), LedgerError> {
        let mut line = serde_json::to_string(row)?;
        line.push('\n');
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("index.jsonl"))?;
        f.write_all(line.as_bytes())?;
        Ok(())
    }

    /// Read the last `limit` index rows (all when `limit` is 0).
    pub fn read_index(&self, limit: usize) -> Result<Vec<IndexRow>, LedgerError> {
        let path = self.dir.join("index.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut rows = Vec::new();
        for line in BufReader::new(File::open(path)?).lines() {
            let line = line?;
            if let Ok(row) = serde_json::from_str::<IndexRow>(&line) {
                rows.push(row);
            }
        }
        if limit > 0 && rows.len() > limit {
            rows.drain(..rows.len() - limit);
        }
        Ok(rows)
    }

    /// Read a session chain. Malformed lines are returned as a broken chain by
    /// [`Ledger::verify_session`]; here they stop the read.
    pub fn read_session(&self, session: &str) -> Result<Vec<LedgerEntry>, LedgerError> {
        let path = self.session_path(session);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for line in BufReader::new(File::open(path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            out.push(serde_json::from_str::<LedgerEntry>(&line)?);
        }
        Ok(out)
    }

    /// Verify one session chain.
    pub fn verify_session(&self, session: &str) -> VerifyReport {
        match self.read_session(session) {
            Ok(entries) => verify_entries(session, &entries),
            Err(e) => VerifyReport {
                session_id: session.to_owned(),
                entries: 0,
                head: None,
                broken_at: Some(0),
                reason: Some(format!("unreadable: {e}")),
            },
        }
    }

    /// Session ids with a chain file, recovered from the first entry of each.
    pub fn sessions(&self) -> Result<Vec<String>, LedgerError> {
        let mut out = Vec::new();
        for entry in fs::read_dir(self.dir.join("sessions"))? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let first = BufReader::new(File::open(&path)?)
                .lines()
                .next()
                .transpose()?;
            if let Some(line) = first
                && let Ok(e) = serde_json::from_str::<LedgerEntry>(&line)
            {
                out.push(e.session_id);
            }
        }
        out.sort();
        Ok(out)
    }
}

/// Filesystem-safe, collision-resistant stem for a session id.
pub fn file_stem(session: &str) -> String {
    let safe: String = session
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    let d = canon::digest_bytes(session.as_bytes());
    let short = d.get(7..15).unwrap_or("00000000");
    format!("{safe}.{short}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ledger() -> (tempfile::TempDir, Ledger) {
        let dir = tempfile::tempdir().unwrap();
        let l = Ledger::open(dir.path()).unwrap();
        (dir, l)
    }

    #[test]
    fn test_ld001_append_and_verify() {
        let (_d, l) = ledger();
        for i in 0..5 {
            l.append(
                "s1",
                LedgerKind::Decision,
                json!({"i": i}),
                "2026-09-26T00:00:00Z",
            )
            .unwrap();
        }
        let r = l.verify_session("s1");
        assert!(r.ok(), "{r:?}");
        assert_eq!(r.entries, 5);
    }

    #[test]
    fn test_ld002_any_edit_breaks_the_chain() {
        let (_d, l) = ledger();
        for i in 0..4 {
            l.append("s1", LedgerKind::Decision, json!({"i": i}), "t")
                .unwrap();
        }
        let path = l.session_path("s1");
        let text = fs::read_to_string(&path).unwrap();
        let tampered = text.replacen("{\"i\":2}", "{\"i\":9}", 1);
        assert_ne!(text, tampered);
        fs::write(&path, tampered).unwrap();
        let r = Ledger::open(l.dir()).unwrap().verify_session("s1");
        assert_eq!(r.broken_at, Some(2));
    }

    #[test]
    fn test_ld003_deleting_a_line_breaks_the_chain() {
        let (_d, l) = ledger();
        for i in 0..4 {
            l.append("s1", LedgerKind::Decision, json!({"i": i}), "t")
                .unwrap();
        }
        let path = l.session_path("s1");
        let text = fs::read_to_string(&path).unwrap();
        let kept: Vec<&str> = text
            .lines()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, l)| l)
            .collect();
        fs::write(&path, kept.join("\n") + "\n").unwrap();
        assert_eq!(
            Ledger::open(l.dir())
                .unwrap()
                .verify_session("s1")
                .broken_at,
            Some(1)
        );
    }

    #[test]
    fn test_ld004_refuses_to_extend_a_broken_chain() {
        let (_d, l) = ledger();
        l.append("s1", LedgerKind::Decision, json!({"i": 0}), "t")
            .unwrap();
        l.append("s1", LedgerKind::Decision, json!({"i": 1}), "t")
            .unwrap();
        let path = l.session_path("s1");
        let text = fs::read_to_string(&path)
            .unwrap()
            .replacen("{\"i\":0}", "{\"i\":5}", 1);
        fs::write(&path, text).unwrap();
        let fresh = Ledger::open(l.dir()).unwrap();
        assert!(matches!(
            fresh.append("s1", LedgerKind::Decision, json!({}), "t"),
            Err(LedgerError::Broken { .. })
        ));
    }

    #[test]
    fn test_ld005_sessions_are_separate_and_resume() {
        let (_d, l) = ledger();
        l.append("a/b", LedgerKind::Decision, json!(1), "t")
            .unwrap();
        l.append("c", LedgerKind::Decision, json!(2), "t").unwrap();
        let reopened = Ledger::open(l.dir()).unwrap();
        let e = reopened
            .append("a/b", LedgerKind::Execution, json!(3), "t")
            .unwrap();
        assert_eq!(e.seq, 1);
        assert_eq!(
            reopened.sessions().unwrap(),
            vec!["a/b".to_owned(), "c".to_owned()]
        );
        assert!(reopened.verify_session("a/b").ok());
    }

    #[test]
    fn test_ld006_stems_do_not_collide() {
        assert_ne!(file_stem("a/b"), file_stem("a_b"));
    }
}
