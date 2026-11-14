//! Commands and declared rows in; guarded batches and decisions out. No IO.
mod backlog_rules;
mod dependency_rules;
mod organization_rules;
mod read_rules;
mod reference_rules;
mod role_rules;
mod statement_plans;
mod status_rules;
mod thread_rules;
mod work_log_rules;

pub use backlog_rules::*;
pub use dependency_rules::*;
pub use organization_rules::*;
pub use read_rules::*;
pub use reference_rules::*;
pub use role_rules::*;
pub use statement_plans::{RulePlan, WriteContext};
pub use status_rules::*;
pub use thread_rules::*;
pub use work_log_rules::*;

#[cfg(test)]
#[path = "rule_fixtures.rs"]
mod fixtures;
