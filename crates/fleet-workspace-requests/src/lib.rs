//! Maps workspace operations to reads and rule inputs.
#![forbid(unsafe_code)]

mod workspace_item_reads;
mod workspace_read_queries;
mod workspace_request_gate;
mod workspace_rule_dispatch;
mod workspace_rule_inputs;

pub use workspace_request_gate::execute;
