//! Drive the owning Host's sender while observing real takeover fixture progress.
use super::super::{
    central_pair::central,
    fixture::BoardFixture,
    takeover_controls::{self as controls, BOUND},
};
use board_central_test_support::{DispatchFaultObservation, ObjectAddress, RuntimeControl};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub async fn copy_project(fixture: &mut BoardFixture) -> FixtureResult {
    tokio::time::timeout(
        BOUND,
        fixture
            .host
            .as_mut()
            .ok_or("owning Host is stopped")?
            .replicate_project(&fixture.project),
    )
    .await?
}

pub async fn held_submit(fixture: &mut BoardFixture) -> FixtureResult<Value> {
    // Keep the existing control client's 20-second guard. Both the cause and
    // its frozen handoff must be copied before the selected submit can arrive.
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            copy_project(fixture).await?;
            let observed = central(fixture)?
                .runtime_control(ObjectAddress::Registry, RuntimeControl::Observe)
                .await?;
            if matches!(observed.dispatch, DispatchFaultObservation::Held { .. }) {
                return controls::held_call(
                    fixture,
                    ObjectAddress::Registry,
                    "central.mailbox-submit",
                )
                .await;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

pub async fn cancelled_after_copy(
    fixture: &mut BoardFixture,
    item: fleet_sync_protocol::ItemId,
) -> FixtureResult {
    let item = item.as_uuid().to_string();
    tokio::time::timeout(BOUND, async {
        loop {
            copy_project(fixture).await?;
            let reply = fixture
                .host()?
                .mcp_reply(
                    "router_show",
                    json!({"caller":{"kind":"humanUser"},"reference":item}),
                )
                .await?;
            if !reply.is_error
                && reply.value.pointer("/mail/status/kind") == Some(&json!("cancelled"))
            {
                assert_eq!(reply.value.pointer("/mail/item"), Some(&json!(item)));
                return FixtureResult::Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}
