//! A completed real Fleet callback decides the budget even without request overlap.
#[path = "support/read_cost_counter.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only the shared alarm statement observer."
)]
mod read_cost_counter;

use board_central_test_support::{
    ObjectAddress, RuntimeControl, RuntimeControlRequest, TestCentral,
};
use message_board::{ActorOrigin, MachineLabel, ServiceId};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Budget assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn an_explicit_empty_fleet_alarm_uses_two_statements() -> TestResult {
    read_cost_counter::install();
    let directory = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer_and_controls(
        directory.path(),
        &ActorOrigin {
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            machine: MachineLabel::try_from("fresh alarm budget".to_owned())?,
        },
        "0192a1b2-c3d4-7000-8000-000000000001"
            .to_owned()
            .try_into()?,
        vec![RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command: RuntimeControl::AutomationClock(serde_json::from_value(json!({
                "kind":"start","now":"2026-10-31T10:00:00Z"
            }))?),
        }],
    )
    .await?;
    let before = central
        .runtime_control(
            ObjectAddress::Fleet,
            RuntimeControl::AutomationClock(serde_json::from_value(json!({"kind":"observe"}))?),
        )
        .await?;
    let observation = read_cost_counter::alarm_read_costs::observe_alarm_costs();
    let invoked = central
        .runtime_control(
            ObjectAddress::Fleet,
            RuntimeControl::AutomationClock(serde_json::from_value(json!({"kind":"invokeAlarm"}))?),
        )
        .await;
    let observed_costs = observation.finish().await;
    let stopped = central.shutdown().await;
    let invoked = invoked?;
    let observed_costs = observed_costs?;
    stopped?;
    let before = before
        .automation_clock
        .ok_or("startup clock observation absent")?;
    let after = invoked
        .automation_clock
        .ok_or("completed callback observation absent")?;
    assert!(
        after.callbacks.len() > before.callbacks.len(),
        "the budget must observe a real new callback"
    );
    assert_eq!(
        serde_json::to_value(&after.callbacks.last().ok_or("callback absent")?.result)?,
        json!({"kind":"completed"})
    );
    let runs = observed_costs
        .by_object
        .get("fleet")
        .ok_or("no Fleet alarm was measured")?;
    assert!(
        !runs.is_empty(),
        "a no-overlap request must not pass this callback proof vacuously"
    );
    eprintln!("FRESH_FLEET_ALARM_COST {observed_costs:?}");
    for statements in runs {
        assert_eq!(*statements, 2, "fresh Fleet alarm statement budget");
    }
    Ok(())
}
