#![expect(
    clippy::unwrap_used,
    reason = "fixture inputs and setup failures must fail the calling test"
)]
use central_engine_model::{Context, EngineTime, IdSource};
use workspace_ids::SessionRef;

pub struct ReplayIds;
impl IdSource for ReplayIds {
    fn next_id(&mut self) -> uuid::Uuid {
        "01890f2e-7b4c-7cc0-98c4-000000000010".parse().unwrap()
    }
}
pub fn time(minute: i64) -> EngineTime {
    EngineTime::from_timestamp(minute * 60, 0).unwrap()
}
pub fn context(minute: i64, ids: &mut ReplayIds) -> Context<'_> {
    Context {
        now: time(minute),
        ids,
    }
}
pub fn target() -> SessionRef {
    SessionRef {
        machine_id: "01890f2e-7b4c-7cc0-98c4-000000000001".parse().unwrap(),
        session_id: "timer-test".to_owned().try_into().unwrap(),
    }
}

pub struct NoIds;
impl IdSource for NoIds {
    fn next_id(&mut self) -> uuid::Uuid {
        uuid::Uuid::from_u128(0x01890f2e7b4c7cc098c4000000000001)
    }
}

pub fn context_now() -> EngineTime {
    EngineTime::from_timestamp(1_794_571_200, 0).unwrap()
}
