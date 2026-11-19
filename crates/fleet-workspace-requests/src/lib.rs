//! Maps workspace operations to reads and rule inputs.
#![forbid(unsafe_code)]

mod workspace_item_reads;
mod workspace_read_queries;
mod workspace_rule_dispatch;
mod workspace_rule_inputs;

pub use workspace_item_reads::{actor_ids, read_answer};
pub use workspace_read_queries::read_statement;
pub use workspace_rule_dispatch::{ResolvedOperation, resolve_command};
pub use workspace_rule_inputs::run_rule;
