//! Generic execution, attempts and administration for Fleet machines.
#![forbid(unsafe_code)]

mod engine;

pub use engine::admin::{
    AttemptRefusal, DiscardOutcome, DiscardStore, EngineHealthFacts, KindHealth, KindRegistry,
    RegisteredDiscard, RetiredItem,
};
pub use engine::admin_failure_class::EngineFailureClass;
pub use engine::alarm_source::{
    DueCutoff, TimedRowOutcome, TimedRows, TimedSource, WholeUnit, WholeUnitOutcome,
};
pub use engine::attempt_statements::{CONTROL_WRITE, delete_attempt, delete_observed_attempt};
pub use engine::control_gate::ControlGate;
pub use engine::failures::FailureOutcome;
pub use engine::handle::HandleOutcome;
pub use engine::port::{Engine, EngineHost};
#[cfg(feature = "test-support")]
pub use engine::receipts::{EngineReceipt, EngineRefusal};
#[cfg(feature = "test-support")]
pub use engine::requests::EngineRequestOutcome;
pub use engine::store_adapter::{
    LoadFailure, StoreAdapter, Versioned, decode_load_failure, load_store_failure, raw_version,
};
