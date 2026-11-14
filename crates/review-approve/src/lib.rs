//! review-approve: the warden's review of a task and the approval of its
//! pulls, in one command.
//!
//! [`plan`] and [`refusal`] decide, with no I/O; [`run`] reads the command
//! line, runs guild as they decide and answers one JSON object.

agent_help::binary!("review-approve");

pub mod plan;
pub mod refusal;
mod run;

pub use run::main;
