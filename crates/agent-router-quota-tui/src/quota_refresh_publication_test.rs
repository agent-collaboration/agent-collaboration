//! The fixture observes the real refresh pipeline only after its completed draw publication.
use super::*;
use crate::{
    QuotaOutcome, QuotaRefreshFailure, QuotaRefreshResult, QuotaRefresher,
    quota_browse_presentation_test::quota_two_account_view_model,
    quota_status_component::QuotaStatusComponent,
    quota_test_support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    },
};
use futures_util::{Stream, StreamExt};
use iocraft::prelude::*;
use iocraft::{ComponentDrawer, Hook};
use std::{
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

struct BeforePublication {
    controller: QuotaFixtureController,
    publications: tokio::sync::mpsc::UnboundedReceiver<QuotaFixtureReadback>,
    checked: Arc<AtomicUsize>,
}
impl Hook for BeforePublication {
    fn pre_component_draw(&mut self, _drawer: &mut ComponentDrawer) {
        let finished = self
            .controller
            .0
            .lock()
            .unwrap()
            .live
            .as_ref()
            .is_some_and(|live| {
                matches!(
                    &*live.context.refresh.read(),
                    crate::quota_control_context::QuotaRefreshState::Finished(_)
                )
            });
        if finished && self.checked.load(Ordering::SeqCst) == 0 {
            while let Ok(publication) = self.publications.try_recv() {
                assert!(
                    publication.refresh_result_published.is_none(),
                    "refresh result appeared before the completed draw publication"
                );
            }
            self.checked.fetch_add(1, Ordering::SeqCst);
        }
    }
}
#[derive(Props)]
struct PublicationRootProps {
    controller: QuotaFixtureController,
    journal: Option<EventJournal>,
    checked: Arc<AtomicUsize>,
    refresher: Option<QuotaRefresher>,
    loader: Option<crate::QuotaStatusViewModelLoader>,
    outcome: Arc<Mutex<Option<QuotaOutcome>>>,
}
#[component]
fn PublicationRoot(
    props: &PublicationRootProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let controller = props.controller.clone();
    let checked = props.checked.clone();
    hooks.use_hook(move || BeforePublication {
        publications: controller.subscribe(),
        controller,
        checked,
    });
    let journal = props.journal.clone().unwrap();
    record_draw(&mut hooks, &journal);
    element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:120usize,height:36usize,
        fixture_controller:Some(props.controller.clone()),quota_refresher:props.refresher.clone(),reload_view_model:props.loader.clone(),
        outcome_sink:Some(props.outcome.clone()),routed_outcome_observer:Some(Handler::from(move|_:Option<QuotaOutcome>|journal.processed(SurfaceSnapshot(Vec::new())))),
        spinner_interval:Duration::from_secs(3600),reload_interval:Duration::from_secs(3600)))
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
async fn signal_while_rendering<T: Stream<Item = Canvas> + ?Sized>(
    mut frames: Pin<&mut T>,
    mut signal: tokio::sync::oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {result=&mut signal=>{result.unwrap();return;},frame=frames.next()=>{assert!(frame.is_some());}}
    }
}
fn no_published_result(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<QuotaFixtureReadback>) {
    while let Ok(publication) = receiver.try_recv() {
        assert!(publication.refresh_result_published.is_none());
    }
}
#[tokio::test]
async fn refresh_result_waits_for_reload_and_completed_publication_under_details() {
    let alpha = quota_two_account_view_model()
        .rows
        .first()
        .unwrap()
        .account_id
        .clone();
    for result in [
        QuotaRefreshResult::Refreshed,
        QuotaRefreshResult::Partial {
            failed: vec![QuotaRefreshFailure {
                account_id: alpha.clone(),
                message: "partial fixture failure".into(),
            }],
        },
        QuotaRefreshResult::Failed {
            failed: vec![QuotaRefreshFailure {
                account_id: alpha.clone(),
                message: "all failed".into(),
            }],
            overall: None,
        },
        QuotaRefreshResult::Failed {
            failed: vec![],
            overall: Some("setup failed".into()),
        },
    ] {
        tokio::time::timeout(Duration::from_secs(5),async {
            // Arrange: hold the actual mapper return and the real reload independently.
            let (refresh_release,refresh_gate)=tokio::sync::oneshot::channel();
            let refresh_gate=Arc::new(Mutex::new(Some(refresh_gate)));
            let returned=result.clone();
            let refresher:QuotaRefresher=Arc::new(move||{let gate=refresh_gate.lock().unwrap().take().unwrap();let result=returned.clone();Box::pin(async move{gate.await.unwrap();result})});
            let (reload_entered,reload_entry)=tokio::sync::oneshot::channel();
            let (reload_release,reload_gate)=tokio::sync::oneshot::channel();
            let reload_gate=Arc::new(Mutex::new(Some((reload_entered,reload_gate))));
            let loader:crate::QuotaStatusViewModelLoader=Arc::new(move||{let (entered,gate)=reload_gate.lock().unwrap().take().unwrap();Box::pin(async move{entered.send(()).unwrap();gate.await.unwrap();Some(quota_two_account_view_model())})});
            let controller=QuotaFixtureController::default();
            let mut publications=controller.subscribe();
            let checked=Arc::new(AtomicUsize::new(0));
            let outcome=Arc::new(Mutex::new(None));
            let (mut driver,config,journal)=EventDriver::new();
            let mut component=element!(PublicationRoot(controller:controller.clone(),journal:Some(journal),checked:checked.clone(),refresher:Some(refresher),loader:Some(loader),outcome:outcome.clone()));
            let mut frames=Box::pin(recording_terminal_render_loop(&mut component,config));
            driver.initial(&mut frames).await;
            driver.drive(&mut frames,InputMode::FrameSeparated,vec![(key(KeyCode::Char('r')),true),(key(KeyCode::Enter),true)],false).await;
            let details=controller.readback().unwrap();
            assert!(matches!(details.captured,Some(QuotaCapturedContext::Details(_))));
            no_published_result(&mut publications);
            // Act: even a mapper result is insufficient while the reload is held.
            refresh_release.send(()).unwrap();
            signal_while_rendering(frames.as_mut(),reload_entry).await;
            no_published_result(&mut publications);
            reload_release.send(()).unwrap();
            let published=loop {tokio::select! {
                publication=publications.recv()=>{let publication=publication.unwrap();if publication.refresh_result_published.is_some(){break publication;}},
                frame=frames.next()=>{assert!(frame.is_some());}
            }};
            // Assert: notice visibility/data changes are not required; real publication is.
            assert_eq!(published.refresh_result_published,Some(result.clone()));
            assert_eq!(published.mount,details.mount);
            assert_eq!(published.captured,details.captured);
            assert_eq!(published.view_model,details.view_model,"all-failed/setup-failed may leave visible facts identical");
            assert!(published.published.as_ref().unwrap().frame>details.published.as_ref().unwrap().frame);
            assert!(checked.load(Ordering::SeqCst)>0,"the pre-draw rejection check must actually run");
            driver.drive(&mut frames,InputMode::FrameSeparated,vec![(key(KeyCode::Esc),true)],false).await;
            let mut until_ack=frames.by_ref().chain(futures_util::stream::pending());
            driver.drive(&mut until_ack,InputMode::Burst,vec![(key(KeyCode::Char('q')),false)],false).await;
            drop(until_ack);
            while frames.next().await.is_some(){}
            assert_eq!(*outcome.lock().unwrap(),Some(QuotaOutcome::Quit));
        }).await.expect("refresh publication and real quit must settle");
    }
}
