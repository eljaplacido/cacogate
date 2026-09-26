//! Per-session state that temporal rules read. Pure data; the daemon owns
//! updates and can rebuild it by replaying the ledger.

use serde::{Deserialize, Serialize};

/// What the kernel knows about a session's history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionState {
    /// Monotonic event counter (executions observed).
    pub events: u64,
    /// Event number of the last executed edit.
    pub last_edit_seq: Option<u64>,
    /// Event number of the last passing test/check run.
    pub last_test_pass_seq: Option<u64>,
    /// Decisions recorded.
    pub decisions: u64,
}

impl SessionState {
    /// An edit executed.
    pub fn record_edit(&mut self) {
        self.events += 1;
        self.last_edit_seq = Some(self.events);
    }

    /// A test/check run passed.
    pub fn record_test_pass(&mut self) {
        self.events += 1;
        self.last_test_pass_seq = Some(self.events);
    }

    /// Any other execution.
    pub fn record_other(&mut self) {
        self.events += 1;
    }

    /// Whether there are edits newer than the last passing test.
    pub fn edits_since_last_test_pass(&self) -> bool {
        match (self.last_edit_seq, self.last_test_pass_seq) {
            (Some(e), Some(t)) => e > t,
            (Some(_), None) => true,
            _ => false,
        }
    }
}
