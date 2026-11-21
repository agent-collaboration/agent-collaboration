//! Recipient routing, notice windows and plans for Central.
#![forbid(unsafe_code)]

mod alarm_input;
mod cadence_request;
mod notice_actor;
mod notice_mailbox;
mod notice_recipients;
mod recipient_cadence;
mod window_items;
mod window_rows;
mod window_statements;
mod window_unit;
mod workspace_notice_plan;

#[doc(hidden)]
pub use fleet_engine::controlled_legacy as __controlled_legacy;

pub use cadence_request::plan_notice_cadence_held;
pub use notice_mailbox::NoticeMailbox;
pub use window_unit::{DueWindowPlan, plan_due_windows_held, window_unit};
pub use workspace_notice_plan::{WorkspaceNoticePlan, plan_workspace_notices_held};
