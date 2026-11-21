//! The schedule job's owned rules, rows and plans.
#![forbid(unsafe_code)]

mod automation_timer;
mod due_queries;
mod finish;
mod request_plan;
mod stored_rows;
mod timed_store;
mod timing;

pub use automation_timer::{AutomationTimer, Effect, Event, State};
pub use due_queries::AUTOMATION_LIST;
#[doc(hidden)]
pub use due_queries::{__controlled_machine, __machine_rows};
pub use finish::{AutomationFinish, automation_finish_store};
pub use request_plan::{disable_automation_read, plan_disable_automation, plan_set_automation};
pub use stored_rows::AutomationRow;
pub use timed_store::AutomationStore;
pub use timing::set_automation_refusal;
