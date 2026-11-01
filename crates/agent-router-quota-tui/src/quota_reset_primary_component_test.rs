//! A checked reset choice and the primary action must describe the same live snapshot.
use super::{provider_filter_key, quota_two_account_view_model};
use crate::{
    QuotaOutcome, QuotaStatusViewOptions,
    test_support::{QuotaFixtureControl, QuotaFixtureOptions, quota_control_fixture},
};
use codex_router_quota_service::quota_reset::reset_session_supervisor::{
    ConfirmationSelection, ResetSessionIntent, ResetSessionPorts, WorkflowPhase,
};
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::time::Duration;

#[tokio::test]
async fn reset_primary_tracks_the_live_choice_before_control_enter_dispatches() {
    let selected_snapshot =
        super::quota_ux_clarity_test::quota_ux_reset_test::confirmation_selection_snapshot;
    let (ports, mut intents, snapshots) =
        ResetSessionPorts::test_channels(selected_snapshot(ConfirmationSelection::No, true));
    let teardown = ports.intent_sender.clone();
    let driver = tokio::spawn(async move {
        let mut observed = Vec::new();
        while let Some(intent) = intents.recv().await {
            observed.push(intent.clone());
            match intent {
                ResetSessionIntent::SelectYes => {
                    snapshots
                        .send(selected_snapshot(ConfirmationSelection::Yes, true))
                        .unwrap();
                }
                ResetSessionIntent::SelectNo => {
                    snapshots
                        .send(selected_snapshot(ConfirmationSelection::No, true))
                        .unwrap();
                }
                ResetSessionIntent::Cancel => {
                    snapshots.send(super::quota_ux_clarity_test::quota_ux_reset_test::clarity_reset_snapshot(WorkflowPhase::Browse)).unwrap();
                }
                ResetSessionIntent::Shutdown => break,
                other => {
                    panic!("choosing No must never consume or begin a new inspection: {other:?}")
                }
            }
        }
        observed
    });
    let mut fixture = quota_control_fixture(QuotaFixtureOptions {
        view_model: quota_two_account_view_model(),
        ports: QuotaStatusViewOptions {
            reset_session_ports: Some(ports),
            ..Default::default()
        },
        width: 120,
        height: 36,
        control: QuotaFixtureControl::ResetPhase,
    });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let frames = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
        &mut fixture.element,
        MockTerminalConfig::with_events(events),
    );
    tokio::pin!(frames);
    crate::quota_frame_observation::next_matching_frame(
        frames.as_mut(),
        Duration::from_secs(2),
        |canvas| canvas.to_string().contains("● Cancel"),
    )
    .await
    .unwrap();
    sender.send(provider_filter_key(KeyCode::Right)).unwrap();
    let yes = crate::quota_frame_observation::next_matching_frame(
        frames.as_mut(),
        Duration::from_secs(2),
        |canvas| canvas.to_string().contains("● Yes"),
    )
    .await
    .unwrap();
    assert!(
        yes.to_string().contains("● Confirm"),
        "the painted Yes snapshot must make Confirm primary before another input: {yes}"
    );
    sender.send(provider_filter_key(KeyCode::Left)).unwrap();
    let no = crate::quota_frame_observation::next_matching_frame(
        frames.as_mut(),
        Duration::from_secs(2),
        |canvas| canvas.to_string().contains("● No"),
    )
    .await
    .unwrap();
    assert!(
        no.to_string().contains("● Cancel") && !no.to_string().contains("● Confirm"),
        "the painted No snapshot must make Cancel primary: {no}"
    );
    let mut enter = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
    enter.modifiers = KeyModifiers::CONTROL;
    sender.send(TerminalEvent::Key(enter)).unwrap();
    crate::quota_frame_observation::next_matching_frame(
        frames.as_mut(),
        Duration::from_secs(2),
        |canvas| {
            let frame = canvas.to_string();
            frame.contains("Not checked") && frame.contains("● Inspect")
        },
    )
    .await
    .unwrap();
    let mut quit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('c'));
    quit.modifiers = KeyModifiers::CONTROL;
    sender.send(TerminalEvent::Key(quit)).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while frames.next().await.is_some() {}
    })
    .await
    .unwrap();
    teardown.send_now(ResetSessionIntent::Shutdown).unwrap();
    assert_eq!(
        driver.await.unwrap(),
        vec![
            ResetSessionIntent::SelectYes,
            ResetSessionIntent::SelectNo,
            ResetSessionIntent::Cancel,
            ResetSessionIntent::Shutdown
        ]
    );
    assert_eq!(*fixture.outcome.lock().unwrap(), Some(QuotaOutcome::Quit));
}
