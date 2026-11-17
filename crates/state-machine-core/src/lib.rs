//! The pure state-machine contract shared by the Router's items and central's
//! engine: state and event tags, the declared transition table, refusals, and
//! the check that a machine's reducer takes exactly its declared transitions.
//!
//! No storage, no runtime, no clock: a transition is a pure function of the
//! current state and one event. Central's engine extends this vocabulary with
//! its own `Machine` trait (scheduling, persisted refusals, an engine context)
//! in central-engine-model.
mod state_machine;
mod transition_check;
mod vocabulary;

pub use state_machine::*;
pub use transition_check::*;
pub use vocabulary::*;
