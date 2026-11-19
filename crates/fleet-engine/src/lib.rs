//! Fleet due work, attempts, and engine administration.
#![forbid(unsafe_code)]

mod engine;

pub use engine::admin::{
    AttemptRefusal, DiscardOutcome, EngineHealthFacts, KindHealth, RetiredItem,
};
pub use engine::admin_records::{EngineFailureClass, EngineKindName};
pub use engine::port::{Engine, EngineHost};
pub use engine::query_registry::alarm_update;

#[cfg(feature = "test-support")]
pub mod test_support {
    pub use crate::engine::machines::owed_reply::{
        Event as OwedEvent, Finish as OwedFinish, OwedReply, OwedReplyRefusal,
        RouterAnswer as OwedAnswer, State as OwedState,
    };
    pub use crate::engine::probe_machine::{
        EngineProbe, ProbeEffect, ProbeEvent, ProbeRefusal, ProbeState,
    };
    pub use crate::engine::receipts::{EngineReceipt, EngineRefusal};
    pub use crate::engine::requests::EngineRequestOutcome;
}
