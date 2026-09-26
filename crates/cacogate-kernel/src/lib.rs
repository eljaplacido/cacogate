//! cacogate kernel: pure decision logic.
//!
//! No I/O, no clock, no process spawning (AGENTS.md article 6). The daemon,
//! CLI replay and conformance runner all call the same functions.

pub mod decide;
pub mod facts;
pub mod packet;
pub mod policy;
pub mod redact;
pub mod session;
pub mod shell;

pub use decide::{DecideInput, combine, decide};
pub use facts::{ActionFacts, KernelEnv, extract};
pub use policy::{CompiledPolicy, Policy};
pub use session::SessionState;
