//! The cacogate daemon: one policy, one ledger, many harness adapters.
//!
//! The imperative shell around the pure kernel: it owns the clock, ids,
//! session state and the ledger. See `docs/01-BLUEPRINT.md` §1.

pub mod config;
pub mod engine;
pub mod server;

pub use config::Config;
pub use engine::Engine;
