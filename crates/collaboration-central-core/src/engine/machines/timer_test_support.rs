use crate::central_timer_rows::{AutomationRow, WakeRow};
use central_engine_model::{Context, EngineTime, IdSource};
use workspace_model::{SessionAddress, central_mail as mail};

pub(super) struct ReplayIds;
impl IdSource for ReplayIds {
    fn next_id(&mut self) -> uuid::Uuid {
        "01890f2e-7b4c-7cc0-98c4-000000000010".parse().unwrap()
    }
}
pub(super) fn time(minute: i64) -> EngineTime {
    EngineTime::from_timestamp(minute * 60, 0).unwrap()
}
pub(super) fn context(minute: i64, ids: &mut ReplayIds) -> Context<'_> {
    Context {
        now: time(minute),
        ids,
    }
}
fn target() -> SessionAddress {
    SessionAddress {
        machine_id: "01890f2e-7b4c-7cc0-98c4-000000000001".parse().unwrap(),
        endpoint: "codex-local".to_owned().try_into().unwrap(),
        session_id: "timer-test".to_owned().try_into().unwrap(),
    }
}
pub(super) fn wake() -> WakeRow {
    WakeRow {
        id: "01890f2e-7b4c-7cc0-98c4-000000000002".parse().unwrap(),
        target: target(),
        due: time(5),
        text: "wake".to_owned().try_into().unwrap(),
        state: mail::WakeState::Active,
    }
}
pub(super) fn automation(interval: bool) -> AutomationRow {
    AutomationRow {
        id: "01890f2e-7b4c-7cc0-98c4-000000000003".parse().unwrap(),
        target: target(),
        timing: if interval {
            agent_automation::TimingRule::Interval { seconds: 300 }
        } else {
            agent_automation::TimingRule::At { at: time(5) }
        },
        anchor: time(0),
        expires_at: time(7 * 24 * 60),
        evaluated: None,
        due: Some(time(5)),
        text: "automation".to_owned().try_into().unwrap(),
        state: mail::AutomationState::Active,
    }
}
