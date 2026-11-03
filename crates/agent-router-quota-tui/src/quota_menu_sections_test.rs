//! Quota menu sections retain the toolbar order and one non-target F6 heading.
use super::*;
use crate::quota_test_support::frame_output::{SharedFrameOutput, dispatch_render_loop};
use crate::quota_test_support::{QuotaInputFrameDriver, current_quota_canvas};
use agent_collaboration_tui::{
    controls::ActionProjections,
    input::input_dispatch_receipt::InputDispatchReceiptSink,
    surfaces::{surface_handle::SurfaceStackHandle, surface_stack::OverlayKind},
};

fn current_menu_frame(stack: SurfaceStackHandle, canvas: &iocraft::Canvas) -> String {
    let snapshot = stack.snapshot();
    assert!(matches!(
        snapshot.overlays().last().unwrap().kind,
        OverlayKind::Menu
    ));
    let published = stack
        .hit_regions()
        .published()
        .expect("actual menu draw published");
    let region = published
        .surfaces
        .iter()
        .find(|region| region.surface == snapshot.top_id())
        .unwrap();
    crate::quota_test_support::published_popup_frame(canvas, &region.cells)
}

#[tokio::test]
async fn quota_menu_sections_keep_toolbar_order_and_one_bound_non_target_group_heading() {
    for (width, height) in [(120usize, 36usize), (76, 30), (48, 24), (48, 12)] {
        let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
        let mounted = observed.clone();
        let observer = Handler::from(move |stack| *mounted.lock().unwrap() = Some(stack));
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let directory = tempfile::tempdir().unwrap();
        let receipt_path = directory.path().join("menu-input.jsonl");
        let sink = InputDispatchReceiptSink::create(&receipt_path).unwrap();
        let floor: crate::WeeklyQuotaFloorSaver = std::sync::Arc::new(|_, _| {
            Box::pin(async { panic!("observing menu sections must not save a floor") })
        });
        let mut component = element!(QuotaStatusComponent(
            view_model:quota_two_account_view_model(),width,height,weekly_floor_saver:Some(floor),
            surface_stack_observer:Some(observer),dispatch_receipt:Some(sink.clone())
        ));
        let bytes = SharedFrameOutput::default();
        let writer = Box::new(sink.frame_writer(bytes.clone()).unwrap());
        let frames = dispatch_render_loop(
            &mut component,
            MockTerminalConfig::with_events(events),
            bytes,
            writer,
        );
        tokio::pin!(frames);
        let mut input = QuotaInputFrameDriver::new(sender, receipt_path);
        current_quota_canvas(frames.as_mut()).await;
        let stack = observed
            .lock()
            .unwrap()
            .expect("actual quota stack mounted");
        let mut canvas = input
            .step(frames.as_mut(), provider_filter_key(KeyCode::F(10)), |_| {
                true
            })
            .await;
        let snapshot = stack.snapshot();
        let top = snapshot.top();
        let projection = ActionProjections::from_registry(&top.actions);
        let expected = [
            "details",
            "filters",
            "pool",
            "options",
            "floor",
            "group-none",
            "group-provider",
            "group-status",
            "login",
            "refresh",
            "help",
            "quit",
        ]
        .map(|name| crate::quota_control_context::action_id(name).unwrap());
        assert_eq!(
            projection
                .menu
                .iter()
                .map(|item| &item.id)
                .collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>(),
            "declared menu-only section order at {width}x{height}"
        );
        assert_eq!(
            projection
                .toolbar
                .iter()
                .map(|item| &item.id)
                .collect::<Vec<_>>(),
            expected[..5].iter().collect::<Vec<_>>(),
            "toolbar actions retain their original order"
        );
        let heading = crate::quota_control_context::action_id("group-cycle").unwrap();
        assert_eq!(
            top.actions
                .definitions()
                .iter()
                .filter(|action| action.id == heading)
                .count(),
            1
        );
        assert!(
            !top.focus.targets().iter().any(|target| target.id
                == agent_collaboration_tui::surfaces::focus_model::TargetId::for_action(&heading)),
            "Group-by heading is never a menu target"
        );
        assert!(
            !projection.menu.iter().any(|item| item.id == heading),
            "F6 owner is rendered as a heading once, never another row"
        );
        drop(snapshot);
        for _ in 0..5 {
            canvas = input
                .step(frames.as_mut(), provider_filter_key(KeyCode::Down), |_| {
                    true
                })
                .await;
        }
        let menu = current_menu_frame(stack, &canvas);
        assert_eq!(
            menu.matches("Group by").count(),
            1,
            "one actual published heading at {width}x{height}: {menu}"
        );
        assert!(menu.contains("F6"), "heading keeps its owner's key: {menu}");
        if height >= 30 {
            for title in ["Details", "Filters", "Pool", "Options", "Floor"] {
                assert_eq!(
                    menu.matches(title).count(),
                    1,
                    "one toolbar-derived row: {menu}"
                );
            }
        }
        input
            .step(frames.as_mut(), provider_filter_key(KeyCode::F(6)), |_| {
                true
            })
            .await;
        assert!(
            stack.snapshot().overlays().is_empty(),
            "heading key closes the menu before invoking its owner"
        );
        let snapshot = stack.snapshot();
        let selected = snapshot
            .top()
            .actions
            .presentations()
            .into_iter()
            .find(|action| {
                action.id == crate::quota_control_context::action_id("group-provider").unwrap()
            })
            .unwrap();
        assert_eq!(
            selected.checked,
            Some(true),
            "actual F6 binding updates the same grouping choice"
        );
        sink.flush_and_check().unwrap();
    }
}
