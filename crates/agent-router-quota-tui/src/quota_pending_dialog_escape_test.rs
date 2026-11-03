//! Pending effects retain the captured dialog through the real shared input owner.
use crate::{
    QuotaOutcome, WeeklyQuotaFloorSaver,
    quota_browse_presentation_test::quota_two_account_view_model,
    quota_fixture_controller::{
        QuotaCapturedContext, QuotaFixtureController, QuotaFixtureReadback,
    },
    quota_status_component::QuotaStatusComponent,
    quota_test_support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    },
    test_support::QuotaFixtureControl,
};
use codex_router_quota_service::quota_reset::reset_session_supervisor::{
    ConfirmationSelection, ResetSessionIntent, ResetSessionPorts, WorkflowPhase,
};
use futures_util::{Stream, StreamExt};
use iocraft::prelude::*;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Props)]
struct PendingDialogRootProps {
    controller: QuotaFixtureController,
    journal: Option<EventJournal>,
    saver: Option<WeeklyQuotaFloorSaver>,
    loader: Option<crate::QuotaStatusViewModelLoader>,
    reset: Option<ResetSessionPorts>,
    outcome: Arc<Mutex<Option<QuotaOutcome>>>,
}

#[component]
fn PendingDialogRoot(
    props: &PendingDialogRootProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let journal = props.journal.clone().unwrap();
    record_draw(&mut hooks, &journal);
    let on_routed = Handler::from(move |_: Option<QuotaOutcome>| {
        // InputOwner has completed this event. State assertions use the real controller.
        journal.processed(SurfaceSnapshot(Vec::new()));
    });
    element!(QuotaStatusComponent(
        view_model:quota_two_account_view_model(),width:120usize,height:48usize,
        fixture_controller:Some(props.controller.clone()),
        fixture_control:Some(if props.reset.is_some() {QuotaFixtureControl::ResetPhase} else {QuotaFixtureControl::Floor}),
        routed_outcome_observer:Some(on_routed),outcome_sink:Some(props.outcome.clone()),
        weekly_floor_saver:props.saver.clone(),reload_view_model:props.loader.clone(),
        reset_intent_sender:props.reset.as_ref().map(|ports|ports.intent_sender.clone()),
        reset_snapshot_receiver:props.reset.as_ref().map(|ports|ports.snapshot_receiver.clone()),
        spinner_interval:Duration::from_secs(3600),reload_interval:Duration::from_secs(3600),
    ))
}

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn control_key(code: char) -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(code));
    event.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(event)
}
fn assert_same_pending_dialog(before: &QuotaFixtureReadback, after: &QuotaFixtureReadback) {
    assert_eq!(
        after.surfaces.top_id(),
        before.surfaces.top_id(),
        "Escape must retain the pending operation's actual overlay"
    );
    assert_eq!(
        after.captured, before.captured,
        "a pending operation must retain its captured account"
    );
    assert_eq!(
        after.browse_account, before.browse_account,
        "input beneath the pending dialog must not select account B"
    );
}
async fn finish<TFrames: Stream<Item = Canvas> + Unpin>(
    driver: &mut EventDriver,
    frames: &mut TFrames,
    outcome: &Arc<Mutex<Option<QuotaOutcome>>>,
) {
    // Quit can return EOF in the same poll that queues its input acknowledgement.
    // Keep that final ACK observable, then consume the real stream's EOF below.
    let mut until_ack = frames.by_ref().chain(futures_util::stream::pending());
    driver
        .drive(
            &mut until_ack,
            InputMode::Burst,
            vec![(key(KeyCode::Char('q')), false)],
            false,
        )
        .await;
    drop(until_ack);
    while frames.next().await.is_some() {}
    assert_eq!(*outcome.lock().unwrap(), Some(QuotaOutcome::Quit));
}
async fn painted<TFrames: Stream<Item = Canvas> + ?Sized>(
    frames: Pin<&mut TFrames>,
    marker: &str,
) -> Canvas {
    crate::quota_frame_observation::next_matching_frame(frames, Duration::from_secs(2), |canvas| {
        canvas.to_string().contains(marker)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn pending_floor_save_rejects_escape_in_the_same_burst() {
    floor_save_keeps_its_captured_dialog(InputMode::Burst).await;
}
#[tokio::test]
async fn pending_floor_save_rejects_escape_after_the_saving_frame() {
    floor_save_keeps_its_captured_dialog(InputMode::FrameSeparated).await;
}
async fn floor_save_keeps_its_captured_dialog(mode: InputMode) {
    tokio::time::timeout(Duration::from_secs(5), async {
        // Arrange: the actual save port remains pending until the test releases it.
        let model = quota_two_account_view_model();
        let first = model.rows.first().unwrap().clone();
        let second = model.rows.get(1).unwrap().clone();
        let expected_percent = first.weekly_quota_floor_percent + 1;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let (entered, mut entries) = tokio::sync::mpsc::unbounded_channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let gate = Arc::new(Mutex::new(Some(released)));
        let saved_calls = calls.clone();
        let saver: WeeklyQuotaFloorSaver = Arc::new(move |account, percent| {
            saved_calls.lock().unwrap().push((account.clone(), percent));
            entered.send((account, percent)).unwrap();
            let released = gate.lock().unwrap().take().expect("only one save may start");
            Box::pin(async move {released.await.unwrap(); Ok(())})
        });
        let mut reloaded = model;
        reloaded.rows.first_mut().unwrap().weekly_quota_floor_percent = expected_percent;
        let loader: crate::QuotaStatusViewModelLoader = Arc::new(move || {
            let model = reloaded.clone();
            Box::pin(async move {Some(model)})
        });
        let controller = QuotaFixtureController::default();
        let outcome = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut component = element!(PendingDialogRoot(controller:controller.clone(),journal:Some(journal),outcome:outcome.clone(),saver:Some(saver),loader:Some(loader)));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut component, config));
        driver.initial(&mut frames).await;
        let original = controller.readback().unwrap();
        assert!(matches!(&original.captured,Some(QuotaCapturedContext::Floor{account_id,..}) if account_id==&first.account_id));
        driver.drive(&mut frames, InputMode::FrameSeparated, vec![(key(KeyCode::Right),true)],false).await;

        // Act: Escape follows Save either before any redraw or after Saving is painted.
        let acknowledgements = driver.drive(&mut frames, mode,
            vec![(key(KeyCode::Enter),true),(key(KeyCode::Esc),false)],true).await;
        if matches!(mode,InputMode::Burst) {
            assert_eq!(acknowledgements.first().unwrap().drawn_frames,
                acknowledgements.get(1).unwrap().drawn_frames,
                "Save and Escape must actually be processed before one redraw");
        }
        let pending = controller.readback().unwrap();
        assert_same_pending_dialog(&original,&pending);
        let save = pending.actions.iter().find(|action|action.id.as_str()=="quota:floor-save").unwrap();
        assert!(!save.availability.is_available(),"the save is genuinely pending");
        loop {
            tokio::select! {
                entry=entries.recv()=> {assert_eq!(entry,Some((first.account_id.clone(),expected_percent)));break;},
                canvas=frames.next()=> {assert!(canvas.is_some(),"save remains mounted");}
            }
        }
        // These real inputs would select and open B if Escape had exposed Browse.
        driver.drive(&mut frames,InputMode::Burst,
            vec![(key(KeyCode::Down),false),(control_key('e'),false)],false).await;
        assert_same_pending_dialog(&original,&controller.readback().unwrap());
        assert_eq!(*calls.lock().unwrap(),vec![(first.account_id.clone(),expected_percent)]);

        // Assert: only A's real completion closes A; B can be opened afterwards.
        release.send(()).unwrap();
        painted(frames.as_mut(),"Quota ·").await;
        while controller.readback().unwrap().captured.is_some() {
            assert!(frames.next().await.is_some());
        }
        let completed=controller.readback().unwrap();
        assert_eq!(completed.view_model.rows.first().unwrap().weekly_quota_floor_percent,expected_percent);
        assert_eq!(completed.view_model.rows.get(1).unwrap().weekly_quota_floor_percent,second.weekly_quota_floor_percent);
        driver.drive(&mut frames,InputMode::FrameSeparated,
            vec![(key(KeyCode::Down),true),(control_key('e'),true)],false).await;
        assert!(matches!(controller.readback().unwrap().captured,
            Some(QuotaCapturedContext::Floor{account_id,..}) if account_id==second.account_id));
        driver.drive(&mut frames,InputMode::FrameSeparated,vec![(key(KeyCode::Esc),true)],false).await;
        assert!(controller.readback().unwrap().captured.is_none(),"normal cancellation is preserved");
        assert_eq!(calls.lock().unwrap().len(),1);
        finish(&mut driver,&mut frames,&outcome).await;
    }).await.expect("held floor save must complete through its real release/quit paths");
}

#[tokio::test]
async fn committing_reset_rejects_escape_in_a_burst() {
    committing_reset_keeps_its_captured_dialog(InputMode::Burst).await;
}
#[tokio::test]
async fn committing_reset_rejects_escape_after_a_frame() {
    committing_reset_keeps_its_captured_dialog(InputMode::FrameSeparated).await;
}
async fn committing_reset_keeps_its_captured_dialog(mode: InputMode) {
    use crate::quota_browse_presentation_test::quota_ux_clarity_test::quota_ux_reset_test::{
        clarity_reset_snapshot, confirmation_selection_snapshot,
    };
    tokio::time::timeout(Duration::from_secs(5),async {
        // Arrange: real intent/watch ports hold the dispatched operation in Committing.
        let snapshot=confirmation_selection_snapshot(ConfirmationSelection::Yes,true);
        let (ports,mut intents,snapshots)=ResetSessionPorts::test_channels(snapshot);
        let controller=QuotaFixtureController::default();
        let outcome=Arc::new(Mutex::new(None));
        let (mut driver,config,journal)=EventDriver::new();
        let saver:WeeklyQuotaFloorSaver=Arc::new(|_,_|panic!("reset must not reach another account's floor save"));
        let mut component=element!(PendingDialogRoot(controller:controller.clone(),journal:Some(journal),outcome:outcome.clone(),saver:Some(saver),reset:Some(ports)));
        let mut frames=Box::pin(recording_terminal_render_loop(&mut component,config));
        driver.initial(&mut frames).await;
        driver.drive(&mut frames,InputMode::FrameSeparated,
            vec![(key(KeyCode::Tab),true),(key(KeyCode::Tab),true),(key(KeyCode::Tab),true)],false).await;
        assert_eq!(controller.readback().unwrap().surfaces.top().focus.focused().unwrap().id.as_str(),"action:quota:reset-advance");
        driver.drive(&mut frames,InputMode::Burst,vec![(key(KeyCode::Enter),false)],false).await;
        loop {
            tokio::select! {
                intent=intents.recv()=> {assert!(matches!(intent,Some(ResetSessionIntent::Confirm { .. })));break;},
                canvas=frames.next()=> {assert!(canvas.is_some());}
            }
        }
        snapshots.send(clarity_reset_snapshot(WorkflowPhase::Committing)).unwrap();
        painted(frames.as_mut(),"Reset request sent").await;
        let committing=controller.readback().unwrap();
        let focused=committing.surfaces.top().focus.focused().unwrap();
        assert_eq!(focused.id.as_str(),"action:quota:reset-advance");
        assert!(!focused.is_enabled(),"actual committing primary is disabled");

        // Act: escaping and attempting underlying account navigation cannot pop it.
        driver.drive(&mut frames,mode,vec![(key(KeyCode::Esc),false),(key(KeyCode::Down),false),(control_key('e'),false)],false).await;
        assert_same_pending_dialog(&committing,&controller.readback().unwrap());
        assert!(matches!(intents.try_recv(),Err(tokio::sync::mpsc::error::TryRecvError::Empty)),
            "Committing must not emit Cancel, Shutdown, another Confirm, or account inspection");

        // Assert: the workflow owns completion; a later terminal result can close.
        snapshots.send(clarity_reset_snapshot(WorkflowPhase::Result)).unwrap();
        painted(frames.as_mut(),"Result").await;
        driver.drive(&mut frames,InputMode::Burst,vec![(key(KeyCode::Esc),false)],false).await;
        assert_eq!(intents.recv().await,Some(ResetSessionIntent::DismissResult));
        snapshots.send(clarity_reset_snapshot(WorkflowPhase::Browse)).unwrap();
        painted(frames.as_mut(),"Options").await;
        // Result dismissal returns to Options; its ordinary close remains available.
        driver.drive(&mut frames,InputMode::FrameSeparated,vec![(key(KeyCode::Esc),true)],false).await;
        assert!(controller.readback().unwrap().captured.is_none());
        finish(&mut driver,&mut frames,&outcome).await;
    }).await.expect("Committing must stay mounted until the workflow supplies its result");
}
