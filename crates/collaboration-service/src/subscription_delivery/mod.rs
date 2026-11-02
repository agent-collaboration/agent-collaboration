mod board_availability;
#[cfg(debug_assertions)]
mod debug_subscription_clock;
mod direct_message_push;
mod native_mail_attempt;
mod reader_delivery_owner;
mod reader_scan_scope;
mod subscription_clock;
#[cfg(debug_assertions)]
pub use debug_subscription_clock::DebugSubscriptionClock;
mod subscription_facts;
pub(crate) use subscription_facts::delivery_retry_delay;
mod subscription_push;
mod subscription_service;
mod subscription_wait;
mod subscription_windows;
pub use subscription_windows::{
    ProjectSubscriptionRecord, ReaderSubscriptionSnapshot, SubscriptionStartupRestoration,
    SubscriptionWindowStore,
};

pub use board_availability::BoardAvailability;
pub use subscription_clock::{SubscriptionClock, SystemSubscriptionClock};
pub use subscription_service::{SubscriptionDeliveryService, SubscriptionDeliveryServiceProps};
pub use subscription_wait::{SubscriptionWaitFilter, SubscriptionWaitResult};

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::owner_fixture::TestClock as TestSubscriptionClock;
