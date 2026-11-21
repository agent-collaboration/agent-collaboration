//! The schedule job's owned rules, rows and plans.
#![forbid(unsafe_code)]

mod central_mail_time;
mod engine;
mod query_registry;
mod request_plan;
mod rows;
mod store;

pub use central_mail_time::set_automation_refusal;
pub use engine::machines::{
    automation_timer::{AutomationTimer, Effect, Event, State},
    lifecycle_finish::{AutomationFinish, AutomationKind, automation_finish_store},
};
pub use query_registry::{__controlled_machine, __machine_rows, AUTOMATION_LIST};
pub use request_plan::{disable_automation_read, plan_disable_automation, plan_set_automation};
pub use rows::AutomationRow;
pub use store::AutomationStore;
