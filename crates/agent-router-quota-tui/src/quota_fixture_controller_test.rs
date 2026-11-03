//! Fixture setup must reach the real list without changing captured dialog ownership.
use super::{provider_filter_key, quota_two_account_view_model};
use crate::{QuotaOutcome, QuotaStatusViewOptions, test_support::*};
use futures_util::{Stream, StreamExt};
use iocraft::{Canvas, prelude::*};
use std::{pin::Pin, sync::Arc, time::Duration};

async fn setup_published<T: Stream<Item = Canvas> + ?Sized>(
    mut frames: Pin<&mut T>,
    pending: QuotaFixturePendingAck,
) -> QuotaFixtureControlAck {
    tokio::time::timeout(Duration::from_secs(2), async {
        let acknowledgement = pending.acknowledged();
        tokio::pin!(acknowledgement);
        loop {
            tokio::select! {
                acknowledgement = &mut acknowledgement => return acknowledgement.unwrap(),
                frame = frames.next() => { frame.expect("real component remains mounted until setup publishes"); }
            }
        }
    }).await.expect("fixture publication must complete")
}

#[tokio::test]
async fn fixture_controller_selection_reaches_real_list_and_reload_keeps_captured_account() {
    let initial = quota_two_account_view_model();
    let alpha = initial.rows[0].account_id.clone();
    let beta = initial.rows[1].account_id.clone();
    let mut fresh = initial.clone();
    fresh.rows.reverse();
    fresh.rows[1].details.weekly_window = "37% fixture reload".to_owned();
    let loader: crate::QuotaStatusViewModelLoader = Arc::new(move || {
        let fresh = fresh.clone();
        Box::pin(async move { Some(fresh) })
    });
    let controller = QuotaFixtureController::default();
    let mut fixture = controlled_quota_fixture(
        QuotaFixtureOptions {
            view_model: initial,
            ports: QuotaStatusViewOptions {
                reload_view_model: Some(loader),
                focused_account_id: Some(alpha.clone()),
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
    let frames = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
        &mut fixture.element,
        MockTerminalConfig::with_events(events),
    );
    tokio::pin!(frames);
    crate::quota_frame_observation::next_matching_frame(
        frames.as_mut(),
        Duration::from_secs(2),
        |canvas| canvas.to_string().contains("Account details"),
    )
    .await
    .unwrap();
    let opened = controller.readback().unwrap();
    assert!(
        matches!(&opened.captured,Some(QuotaCapturedContext::Details(target)) if target.account_id==alpha)
    );
    let original_focus = opened.surfaces.top().focus.current().cloned();
    let selected = controller
        .select_browse_account(opened.mount, beta.clone())
        .unwrap();
    let acknowledgement = setup_published(frames.as_mut(), selected).await;
    assert_eq!(
        acknowledgement.operation,
        QuotaFixtureOperation::SelectBrowseAccount
    );
    assert_eq!(acknowledgement.readback.browse_account, Some(beta.clone()));
    assert_eq!(
        acknowledgement.readback.list_selection.as_deref(),
        Some(beta.as_str()),
        "the actual ListSelectionControl consumes the parent selection"
    );
    assert!(
        matches!(&acknowledgement.readback.captured,Some(QuotaCapturedContext::Details(target)) if target.account_id==alpha)
    );
    assert_eq!(
        acknowledgement.readback.surfaces.top().focus.current(),
        original_focus.as_ref(),
        "fixture setup does not repair or move keyboard focus"
    );
    assert!(
        acknowledgement
            .readback
            .published
            .as_ref()
            .is_some_and(|frame| frame.frame > opened.published.as_ref().unwrap().frame)
    );
    let reloaded = controller.reload_view_model(opened.mount).await.unwrap();
    let acknowledgement = setup_published(frames.as_mut(), reloaded).await;
    assert_eq!(
        acknowledgement.operation,
        QuotaFixtureOperation::ReloadViewModel
    );
    assert_eq!(
        acknowledgement.readback.list_selection.as_deref(),
        Some(beta.as_str())
    );
    assert!(
        matches!(&acknowledgement.readback.captured,Some(QuotaCapturedContext::Details(target)) if target.account_id==alpha)
    );
    assert!(acknowledgement.readback.view_model.rows.iter().any(|row|row.account_id==alpha&&row.details.weekly_window=="37% fixture reload"));
    assert!(
        controller
            .select_browse_account(opened.mount + 1, alpha)
            .is_err(),
        "operations cannot target a different view generation"
    );
    sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
    crate::quota_frame_observation::next_matching_frame(
        frames.as_mut(),
        Duration::from_secs(2),
        |canvas| !canvas.to_string().contains("Account details"),
    )
    .await
    .unwrap();
    sender
        .send(provider_filter_key(KeyCode::Char('q')))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while frames.next().await.is_some() {}
    })
    .await
    .unwrap();
    assert_eq!(*fixture.outcome.lock().unwrap(), Some(QuotaOutcome::Quit));
}
