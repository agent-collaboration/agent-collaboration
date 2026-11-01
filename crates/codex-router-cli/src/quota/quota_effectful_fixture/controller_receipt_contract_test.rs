use super::*;
use agent_router_quota_tui::{
    QuotaOutcome, QuotaStatusViewOptions,
    test_support::{QuotaFixtureControl, QuotaFixtureOptions, controlled_quota_fixture},
};
use futures_util::{Stream, StreamExt};
use iocraft::{Canvas, prelude::*};
use std::{pin::Pin, time::Duration};
async fn frame<T: Stream<Item = Canvas> + ?Sized>(mut frames: Pin<&mut T>, marker: &str) -> Canvas {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let frame = frames.next().await.unwrap();
            if frame.to_string().contains(marker) {
                return frame;
            }
        }
    })
    .await
    .unwrap()
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
#[tokio::test]
async fn controller_setup_receipt_from_actual_list_is_never_a_user_input_receipt() {
    let state = OwnedQuotaState::create(Scenario::CapturedActions)
        .await
        .unwrap();
    let model = state.model(120).await.unwrap();
    let beta = model.rows[1].account_id.clone();
    let controller = QuotaFixtureController::default();
    let mut fixture = controlled_quota_fixture(
        QuotaFixtureOptions {
            view_model: model,
            ports: QuotaStatusViewOptions {
                focused_account_id: Some(state.target.account_id.clone()),
                ..Default::default()
            },
            width: 120,
            height: 36,
            control: QuotaFixtureControl::Details,
        },
        QuotaFixtureView {
            controller: controller.clone(),
            ..Default::default()
        },
    );
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let frames = recording_backend::recording_terminal_render_loop(
        &mut fixture.element,
        MockTerminalConfig::with_events(events),
    );
    tokio::pin!(frames);
    frame(frames.as_mut(), "Account details").await;
    let mounted = controller.readback().unwrap();
    let pending = controller
        .select_browse_account(mounted.mount, beta.clone())
        .unwrap();
    let acknowledgement = tokio::time::timeout(Duration::from_secs(2), async {
        let ack = pending.acknowledged();
        tokio::pin!(ack);
        loop {
            tokio::select! {ack=&mut ack=>break ack.unwrap(),next=frames.next()=>{next.unwrap();}}
        }
    })
    .await
    .unwrap();
    let wire = serde_json::to_value(receipts::FixtureControlReceipt::from_ack(
        1,
        &acknowledgement,
    ))
    .unwrap();
    assert_eq!(
        wire["kind"], "fixture-control-ack",
        "a setup acknowledgement cannot impersonate user dispatch"
    );
    assert!(
        wire.get("sequence").is_none()
            && wire.get("event").is_none()
            && wire.get("surface").is_none(),
        "only InputOwner can emit that receipt schema"
    );
    assert_eq!(wire["readback"]["listSelection"], beta.as_str());
    assert_eq!(
        wire["readback"]["captured"]["accountId"],
        state.target.account_id.as_str()
    );
    sender.send(key(KeyCode::Esc)).unwrap();
    frame(frames.as_mut(), "Quota ·").await;
    sender.send(key(KeyCode::Char('q'))).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while frames.next().await.is_some() {}
    })
    .await
    .unwrap();
    assert_eq!(*fixture.outcome.lock().unwrap(), Some(QuotaOutcome::Quit));
}
#[tokio::test]
async fn remove_cancel_fixture_ctrl_enter_returns_to_details_with_zero_owner_effects() {
    let state = OwnedQuotaState::create(Scenario::RemoveCancel)
        .await
        .unwrap();
    let model = state.model(120).await.unwrap();
    let (_output, events) = events();
    let effects = AccountEffects::default();
    let (queue, runner) = agent_router_quota_tui::quota_account_command_runner(effects.commands(
        state.root().to_owned(),
        Scenario::RemoveCancel,
        events,
    ));
    let task = tokio::spawn(runner);
    let controller = QuotaFixtureController::default();
    let mut fixture = controlled_quota_fixture(
        QuotaFixtureOptions {
            view_model: model,
            ports: QuotaStatusViewOptions {
                focused_account_id: Some(state.target.account_id.clone()),
                account_commands: Some(queue.clone()),
                ..Default::default()
            },
            width: 120,
            height: 36,
            control: QuotaFixtureControl::Details,
        },
        QuotaFixtureView {
            controller: controller.clone(),
            ..Default::default()
        },
    );
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let frames = recording_backend::recording_terminal_render_loop(
        &mut fixture.element,
        MockTerminalConfig::with_events(events),
    );
    tokio::pin!(frames);
    frame(frames.as_mut(), "Account details").await;
    sender.send(key(KeyCode::Enter)).unwrap();
    frame(frames.as_mut(), "Remove account…").await;
    let map = controller.readback().unwrap();
    let target = map
        .published
        .as_ref()
        .unwrap()
        .targets
        .iter()
        .find(|target| {
            target.surface == map.surfaces.top_id()
                && target.target.as_str().contains("quota:remove")
        })
        .unwrap();
    let (column, row) = target.cells[target.cells.len() / 2];
    sender
        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
        )))
        .unwrap();
    frame(frames.as_mut(), "Confirm removal").await;
    let mut enter = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
    enter.modifiers = KeyModifiers::CONTROL;
    sender.send(TerminalEvent::Key(enter)).unwrap();
    frame(frames.as_mut(), "Account details").await;
    sender.send(key(KeyCode::Esc)).unwrap();
    frame(frames.as_mut(), "Quota ·").await;
    sender.send(key(KeyCode::Char('q'))).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while frames.next().await.is_some() {}
    })
    .await
    .unwrap();
    queue.close();
    assert!(task.await.unwrap().is_empty());
    assert!(effects.records.lock().unwrap().is_empty());
    assert_eq!(read_accounts(state.root()).await.unwrap().len(), 2);
    assert_eq!(*fixture.outcome.lock().unwrap(), Some(QuotaOutcome::Quit));
}
