//! Organization-scoped execution for Fleet features using injected host ports.
#![forbid(unsafe_code)]

mod alarm_decision;
mod alarm_schedule;
mod central_turn;
mod clock_ids;
mod due_batch_state;
mod fleet_runtime;
#[cfg(feature = "test-support")]
mod pause_points;
pub mod stored_values;
#[cfg(feature = "test-support")]
mod test_faults;
pub mod workspace_decode_error;
pub mod workspace_request_log;
mod workspace_turns;

pub use alarm_decision::engine_alarm_decider;
pub use alarm_schedule::{AlarmDeadline, AlarmPublicationStatus, AlarmSchedule};
pub use central_turn::TurnGuard;
pub use clock_ids::ClockIds;
#[cfg(feature = "test-support")]
pub use due_batch_state::DueUnitStart;
pub use due_batch_state::{DueBatchSnapshot, DueBatchState};
pub use fleet_runtime::FleetRuntime;
#[cfg(feature = "test-support")]
pub use pause_points::{HeldPause, PausePoint, PausePoints};
#[cfg(feature = "test-support")]
pub use test_faults::{EngineCommitFault, EngineCommitFaultKind, EngineReadFault, TestFaults};
pub use workspace_turns::{WorkspaceTurn, WorkspaceTurns};
