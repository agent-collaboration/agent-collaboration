//! A separate real mutation makes maintenance observable after the foreground cost window.
use super::{
    board_fixture::{BoardFixture, TestError, accepted},
    read_cost_counter::alarm_read_costs::{AlarmRunCosts, observe_alarm_costs},
};
use serde_json::json;

pub async fn probe_fixed_maintenance(fixture: &BoardFixture) -> Result<AlarmRunCosts, TestError> {
    let alarms = observe_alarm_costs();
    let mut request = fixture.header();
    request.as_object_mut().ok_or("probe header")?.extend(
        json!({"workspaceId":fixture.workspace,"name":"maintenance probe","description":"fixed owner SQL probe"})
            .as_object().ok_or("probe fields")?.clone(),
    );
    accepted(&fixture.api.call("workspace_update", request).await?);
    // This uses the existing span-completion watch, never a sleep or a made-up
    // zero profile. Start and completion share the original twenty-second bound.
    alarms.finish_after_new_alarm().await
}
