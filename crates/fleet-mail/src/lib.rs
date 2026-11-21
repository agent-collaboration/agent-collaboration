//! Owns FleetRuntime mailbox rows, delivery and timed message transitions.
#![forbid(unsafe_code)]

mod acknowledgement;
mod delivery_reads;
mod direct_message;
mod direct_message_rows;
mod direct_message_store;
mod direct_status;
mod due_queries;
pub mod message_deadline;
mod message_effects;
mod message_insert;
mod queued_notices;
mod request_plan;
mod sender_resolution;
mod stored_messages;
mod system_message_expiry;
mod system_message_expiry_store;

#[doc(hidden)]
pub use fleet_engine::{
    controlled_machine as __controlled_machine, machine_rows as __machine_rows,
};

pub use acknowledgement::settle_delivery_ack_held;
pub use delivery_reads::{prepare_windowed_delivery_held, windowed_welcome_page_held};
pub use direct_message::{
    DirectMessage, DirectMessageRefusal, DirectMessageSubject, Effect as DirectMessageEffect,
    Event as DirectMessageEvent, State as DirectMessageState,
};
pub use direct_message_store::DirectMessageStore;
pub use direct_status::direct_message_status_held;
pub use due_queries::{DIRECT_MESSAGE_LIST, SYSTEM_MESSAGE_EXPIRY_LIST};
pub use message_insert::insert_message;
pub use queued_notices::{plan_cancel_queued_notices_held, validate_queued_notices_held};
pub use request_plan::{DirectSendPlan, plan_direct_send, plan_sourced_notice};
pub use stored_messages::queued_for_session_held;
pub use system_message_expiry::{
    ExpireAt, ExpiryRefusal, State as SystemExpiryState, SystemMessageExpiry,
};
pub use system_message_expiry_store::SystemMessageExpiryStore;
