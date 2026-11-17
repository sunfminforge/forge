//! gate-head: whether `guild gate` is green on exactly the worktree's HEAD
//! tree, in one call.
//!
//! [`decide`] decides, with no I/O; [`run`] reads the command line, runs
//! `guild gate` ([`guild`]) and reads git ([`git`]) as it needs, and prints
//! what `decide` makes of them.

agent_help::binary!("gate-head");

pub mod decide;
mod git;
mod guild;
mod run;

pub use run::main;
