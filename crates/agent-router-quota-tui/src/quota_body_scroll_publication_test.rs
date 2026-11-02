//! Published body witnesses follow the real dialog and its measured scroll owner.
use super::*;
use crate::{
    quota_browse_presentation_test::quota_two_account_view_model,
    quota_status_component::QuotaStatusComponent,
    quota_test_support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        frame_grid::FrameGrid,
        recording_backend::recording_terminal_render_loop,
    },
    test_support::QuotaFixtureControl,
};
use futures_util::{Stream, StreamExt};
use iocraft::{Canvas, prelude::*};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

#[derive(Props)]
struct BodyScrollFixtureProps {
    controller: QuotaFixtureController,
    journal: Option<EventJournal>,
    width: usize,
    height: usize,
    control: QuotaFixtureControl,
    unpublished_reads: Arc<AtomicUsize>,
}
#[component]
fn BodyScrollFixture(
    props: &BodyScrollFixtureProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let journal = props.journal.clone().unwrap();
    record_draw(&mut hooks, &journal);
    let controller = props.controller.clone();
    let unpublished = props.unpublished_reads.clone();
    let mut view_model = quota_two_account_view_model();
    if props.control == QuotaFixtureControl::Details && props.width == 48 && props.height == 12 {
        // The native login journey pins its extra login action, leaving one body row.
        view_model.rows[0].login_state =
            codex_router_quota_service::quota_display_model::QuotaLoginState::Required;
    }
    element!(QuotaStatusComponent(
        view_model,width:props.width,height:props.height,
        fixture_control:Some(props.control),fixture_controller:Some(props.controller.clone()),
        routed_outcome_observer:Some(Handler::from(move|_:Option<crate::QuotaOutcome>|{
            let current = controller.readback().unwrap();
            if current.published.is_none() {
                assert!(current.body_scroll.is_none(), "an invalidated frame cannot export a current body witness");
                unpublished.fetch_add(1, Ordering::SeqCst);
            }
            journal.processed(SurfaceSnapshot(Vec::new()));
        })),
        spinner_interval:Duration::from_secs(3600),reload_interval:Duration::from_secs(3600)
    ))
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn fixture(
    controller: &QuotaFixtureController,
    journal: EventJournal,
    width: usize,
    height: usize,
    control: QuotaFixtureControl,
    unpublished_reads: Arc<AtomicUsize>,
) -> AnyElement<'static> {
    element!(BodyScrollFixture(controller:controller.clone(),journal:Some(journal),width,height,control,unpublished_reads)).into_any()
}
async fn initial_body(
    driver: &mut EventDriver,
    frames: &mut (impl Stream<Item = Canvas> + Unpin),
    controller: &QuotaFixtureController,
) {
    tokio::time::timeout(Duration::from_secs(2), async {
        driver.initial(frames).await;
        while !controller.last_published().is_some_and(|readback| {
            readback.captured.is_some()
                && readback.published.is_some_and(|frame| {
                    frame.targets.iter().any(|target| {
                        target.target.as_str() == "action:quota:dialog-body"
                            && !target.cells.is_empty()
                    })
                })
        }) {
            let canvas = frames.next().await.expect("active dialog body draw");
            driver.frames.push(FrameGrid::from_canvas(&canvas).unwrap());
        }
    })
    .await
    .unwrap();
}
fn published_body(controller: &QuotaFixtureController) -> QuotaBodyScrollSnapshot {
    let readback = controller.readback().unwrap();
    let body = readback
        .body_scroll
        .expect("the completed active body publication needs a scroll witness");
    assert_eq!(body.mount, readback.mount);
    assert_eq!(body.surface, readback.surfaces.top_id());
    assert_eq!(body.publication_frame, readback.published.unwrap().frame);
    body
}

#[tokio::test]
async fn one_row_details_body_exports_the_completed_surface_and_frame() {
    // Arrange: the same short Details geometry that cannot show the whole body at once.
    let controller = QuotaFixtureController::default();
    let (mut driver, terminal, journal) = EventDriver::new();
    let mut ui = fixture(
        &controller,
        journal,
        48,
        12,
        QuotaFixtureControl::Details,
        Arc::default(),
    );
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, terminal));
    // Act: wait on real published Details geometry, independently of the new witness.
    initial_body(&mut driver, &mut frames, &controller).await;
    // Assert: measured rows and publication identity are available at the reader's boundary.
    let body = published_body(&controller);
    assert_eq!(body.position.viewport_height, 1);
    let frame = controller.readback().unwrap().published.unwrap();
    let region = frame
        .targets
        .iter()
        .find(|target| {
            target.surface == body.surface && target.target.as_str() == "action:quota:dialog-body"
        })
        .unwrap();
    let painted_rows = region
        .cells
        .iter()
        .map(|(_, row)| *row)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        painted_rows.len(),
        1,
        "the witness describes the actual painted ScrollArea"
    );

    assert!(body.position.natural_height > 1);
    assert!(body.position.offset <= body.position.maximum());
    assert_eq!(body.position.maximum(), body.position.natural_height - 1);
}

#[tokio::test]
async fn down_progresses_each_published_body_row_and_stops_at_the_real_maximum() {
    // Arrange: a one-row viewport and causal acknowledgements for accepted/no-op input.
    let controller = QuotaFixtureController::default();
    let unpublished = Arc::new(AtomicUsize::new(0));
    let (mut driver, terminal, journal) = EventDriver::new();
    let mut ui = fixture(
        &controller,
        journal,
        48,
        12,
        QuotaFixtureControl::Details,
        unpublished.clone(),
    );
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, terminal));
    initial_body(&mut driver, &mut frames, &controller).await;
    let mut initial = published_body(&controller);
    assert!(initial.position.maximum() > 1);
    if initial.position.offset > 0 {
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(key(KeyCode::Home), true)],
                false,
            )
            .await;
        initial = published_body(&controller);
    }
    assert_eq!(initial.position.offset, 0);
    let mut previous_frame = initial.publication_frame;
    // Act / Assert: each Down reaches the next row through the real input and draw owners.
    for expected_offset in 1..=initial.position.maximum() {
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(key(KeyCode::Down), true)],
                false,
            )
            .await;
        let body = published_body(&controller);
        assert_eq!(body.position.offset, expected_offset);
        assert_eq!(body.position.maximum(), initial.position.maximum());
        assert!(body.publication_frame > previous_frame);
        previous_frame = body.publication_frame;
    }
    // The final no-op is acknowledged without inventing or requiring another draw.
    driver
        .drive(
            &mut frames,
            InputMode::FrameSeparated,
            vec![(key(KeyCode::Down), false)],
            false,
        )
        .await;
    let end = published_body(&controller);
    assert_eq!(end.position.offset, end.position.maximum());
    assert!(end.publication_frame >= previous_frame);
    assert!(
        unpublished.load(Ordering::SeqCst) > 0,
        "the test must also reach input-before-publication"
    );
}

#[tokio::test]
async fn fitting_filter_body_exports_zero_maximum_without_requiring_a_scroll() {
    // Arrange: the real three-field Filters body fits its content-sized viewport.
    let controller = QuotaFixtureController::default();
    let (mut driver, terminal, journal) = EventDriver::new();
    let mut ui = fixture(
        &controller,
        journal,
        120,
        80,
        QuotaFixtureControl::Filters,
        Arc::default(),
    );
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, terminal));
    // Act: publish the real body.
    initial_body(&mut driver, &mut frames, &controller).await;
    // Assert: max zero is a genuine completed state, not missing geometry.
    let body = published_body(&controller);
    assert!(body.position.viewport_height > 0);
    assert!(body.position.natural_height <= body.position.viewport_height);
    assert_eq!(body.position.offset, 0);
    assert_eq!(body.position.maximum(), 0);
}

#[tokio::test]
async fn screen_help_uses_its_active_body_and_browse_has_no_dialog_body() {
    // Arrange: Browse owns a selection list, not a DialogFrame scrolling body.
    let controller = QuotaFixtureController::default();
    let (mut driver, terminal, journal) = EventDriver::new();
    let mut ui = fixture(
        &controller,
        journal,
        48,
        12,
        QuotaFixtureControl::Browse,
        Arc::default(),
    );
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, terminal));
    driver.initial(&mut frames).await;
    assert!(controller.readback().unwrap().body_scroll.is_none());
    // Act: Help opens its own actual DialogFrame.
    driver
        .drive(
            &mut frames,
            InputMode::FrameSeparated,
            vec![(key(KeyCode::F(1)), true)],
            false,
        )
        .await;
    // Assert: Help's overlay and completed frame own the witness.
    let body = published_body(&controller);
    assert!(matches!(body.surface, SurfaceId::Overlay(_)));
    assert!(body.position.viewport_height > 0);
    assert!(body.position.natural_height > 0);
}
