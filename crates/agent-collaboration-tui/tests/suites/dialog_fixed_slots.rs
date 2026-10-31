#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, InputMode},
        fixed_slots_scene::{FixedSlotsScene, action, target},
        frame_grid::FrameGrid,
        list_scene::key,
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::focus_model::TargetId;
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};

    fn row_of(frame: &FrameGrid, text: &str) -> usize {
        (0..frame.height)
            .find(|row| frame.row_text(*row).contains(text))
            .unwrap_or_else(|| panic!("missing {text}:\n{}", frame.round_frame_text()))
    }
    fn fixed_rows(frame: &FrameGrid) -> (usize, usize, usize) {
        let controls = row_of(frame, "Credits");
        assert!(frame.row_text(controls).contains("Resets"));
        (
            controls,
            row_of(frame, "Credit balance refreshed."),
            row_of(frame, "Close"),
        )
    }
    async fn pinned_through_scroll(width: u16, height: u16) {
        let (mut driver, config, journal) = EventDriver::new();
        let handle = Arc::new(Mutex::new(None));
        let mut ui = element!(FixedSlotsScene(journal,width,height,rows:40u16,controls:true,status:true,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let fixture = handle.lock().unwrap().clone().unwrap();
            let before = driver.frames.last().unwrap();
            eprintln!(
                "PINNED {width}x{height} INITIAL:\n{}",
                before.round_frame_text()
            );
            let fixed = fixed_rows(before);
            assert_eq!(
                row_of(before, "BODY_00"),
                fixed.0 + 2,
                "one R12 gap after controls"
            );
            assert_eq!(fixed.2, fixed.1 + 2, "one R12 gap before actions");
            let position = fixture.scroll.position();
            assert!(
                position.viewport_height >= 1,
                "even the shortest dialog must retain a usable scrolling body"
            );
            if height > 12 {
                assert!(
                    position.viewport_height >= 3,
                    "all feasible body rows remain available at {width}x{height}"
                );
            }
            assert_eq!(
                fixed.1,
                usize::try_from(
                    i32::from(position.viewport_top) + i32::from(position.viewport_height) + 1
                )
                .unwrap(),
                "one R12 gap before fixed status"
            );
            fixture
                .stack
                .focus(fixture.stack.snapshot().top_id(), &target("body"));
            driver.initial(&mut frames).await;
            let mut pages = vec![driver.frames.last().unwrap().round_frame_text()];
            for _ in 0..position.maximum().div_ceil(position.viewport_height) {
                driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(key(KeyCode::PageDown), true)],
                        true,
                    )
                    .await;
                let frame = driver.frames.last().unwrap();
                assert_eq!(
                    fixed_rows(frame),
                    fixed,
                    "fixed slots and actions must not scroll at {width}x{height}"
                );
                pages.push(frame.round_frame_text());
            }
            assert_eq!(
                fixture.scroll.position().offset,
                fixture.scroll.position().maximum()
            );
            eprintln!(
                "PINNED {width}x{height} END:\n{}",
                driver.frames.last().unwrap().round_frame_text()
            );
            let all = pages.join("\n");
            for row in 0..40 {
                assert!(
                    all.contains(&format!("BODY_{row:02}")),
                    "body row {row} is unreachable at {width}x{height}"
                );
            }
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::Home), true)],
                    true,
                )
                .await;
            assert_eq!(fixture.scroll.position().offset, 0);
            assert_eq!(fixed_rows(driver.frames.last().unwrap()), fixed);
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn both_slots_stay_visible_and_every_body_row_is_reachable_at_32x24() {
        pinned_through_scroll(32, 24).await;
    }
    #[tokio::test]
    async fn both_slots_stay_visible_and_every_body_row_is_reachable_at_48x12() {
        pinned_through_scroll(48, 12).await;
    }
    #[tokio::test]
    async fn fixed_slots_keep_the_body_usable_at_the_other_four_standard_grids() {
        for (width, height) in [(120, 36), (100, 24), (76, 30), (40, 24)] {
            pinned_through_scroll(width, height).await;
        }
    }

    #[tokio::test]
    async fn pinned_status_is_visible_without_a_controls_slot() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui =
            element!(FixedSlotsScene(journal,width:32u16,height:24u16,rows:40u16,status:true));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let frame = driver.frames.last().unwrap();
            assert_eq!(
                row_of(frame, "Close"),
                row_of(frame, "Credit balance refreshed.") + 2
            );
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn fixed_slots_are_subtracted_before_header_can_take_the_third_body_row() {
        let (mut driver, config, journal) = EventDriver::new();
        let handle = Arc::new(Mutex::new(None));
        let mut ui = element!(FixedSlotsScene(journal,width:32u16,height:15u16,rows:40u16,controls:true,status:true,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            fixed_rows(driver.frames.last().unwrap());
            assert_eq!(
                handle
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .scroll
                    .position()
                    .viewport_height,
                3,
                "compact header preserves all three feasible body rows"
            );
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn focus_order_is_header_then_pinned_controls_then_body_and_footer() {
        let (mut driver, config, journal) = EventDriver::new();
        let handle = Arc::new(Mutex::new(None));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut ui = element!(FixedSlotsScene(journal,width:76u16,height:30u16,rows:40u16,controls:true,status:true,handle:handle.clone(),calls:calls.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            fixed_rows(driver.frames.last().unwrap());
            let fixture = handle.lock().unwrap().clone().unwrap();
            for (index, expected) in [
                TargetId::for_action(&action("header")),
                TargetId::for_action(&action("resets")),
                TargetId::for_action(&action("credits")),
                target("body"),
                TargetId::for_action(&action("close")),
            ]
            .into_iter()
            .enumerate()
            {
                if index > 0 {
                    driver
                        .drive(
                            &mut frames,
                            InputMode::FrameSeparated,
                            vec![(key(KeyCode::Tab), true)],
                            true,
                        )
                        .await;
                }
                assert_eq!(
                    fixture.stack.snapshot().top().focus.current(),
                    Some(&expected)
                );
                if index < 3 {
                    driver
                        .drive(
                            &mut frames,
                            InputMode::FrameSeparated,
                            vec![(key(KeyCode::Enter), false)],
                            false,
                        )
                        .await;
                }
            }
            assert_eq!(*calls.lock().unwrap(), vec!["header", "resets", "credits"]);
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn content_height_counts_both_fixed_slots_and_each_gap_without_filler() {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(FixedSlotsScene(journal,width:76u16,height:30u16,rows:3u16,content_height:true,controls:true,status:true));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let frame = driver.frames.last().unwrap();
            let fixed = fixed_rows(frame);
            assert_eq!(row_of(frame, "BODY_00"), fixed.0 + 2);
            assert_eq!(fixed.1, row_of(frame, "BODY_02") + 2);
            assert_eq!(fixed.2, fixed.1 + 2);
            assert_eq!(
                frame.round_frame_text().lines().count(),
                13,
                "border2+header1+contextgap1+controls1+gap1+body3+gap1+status1+footer2"
            );
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn multiline_fixed_slots_use_their_measured_height_for_content_and_header_budget() {
        for content in [false, true] {
            let (mut driver, config, journal) = EventDriver::new();
            let handle = Arc::new(Mutex::new(None));
            let mut ui = element!(FixedSlotsScene(journal,width:if content {76u16}else{32u16},height:if content {30u16}else{17u16},rows:if content {3u16}else{40u16},controls:true,status:true,tall_slots:true,content_height:content,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let frame = driver.frames.last().unwrap();
                fixed_rows(frame);
                assert_eq!(
                    row_of(frame, "BODY_00"),
                    row_of(frame, "Controls detail") + 2
                );
                assert_eq!(row_of(frame, "Close"), row_of(frame, "Status detail") + 2);
                assert_eq!(
                    handle
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .scroll
                        .position()
                        .viewport_height,
                    3
                );
                if content {
                    assert_eq!(frame.round_frame_text().lines().count(), 15);
                }
            })
            .await
            .unwrap();
        }
    }
    #[tokio::test]
    async fn pointer_activates_the_pinned_segment_once_after_body_scroll_without_moving_the_viewport()
     {
        for (width, height) in [(32u16, 24u16), (48, 12)] {
            let (mut driver, config, journal) = EventDriver::new();
            let handle = Arc::new(Mutex::new(None));
            let calls = Arc::new(Mutex::new(Vec::new()));
            let mut ui = element!(FixedSlotsScene(journal,width,height,rows:40u16,controls:true,status:true,handle:handle.clone(),calls:calls.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.lock().unwrap().clone().unwrap();
                let fixed = fixed_rows(driver.frames.last().unwrap());
                assert_eq!(fixed.2, fixed.1 + 2, "one R12 gap before actions");
                assert!(
                    fixture.scroll.position().viewport_height >= 1,
                    "the pointer journey needs a usable body before scrolling"
                );
                fixture
                    .stack
                    .focus(fixture.stack.snapshot().top_id(), &target("body"));
                driver.initial(&mut frames).await;
                driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(key(KeyCode::End), true)],
                        true,
                    )
                    .await;
                let offset = fixture.scroll.position().offset;
                assert_eq!(offset, fixture.scroll.position().maximum());
                let credit = TargetId::for_action(&action("credits"));
                let map = fixture.stack.hit_regions().published().unwrap();
                let point = *map
                    .targets
                    .iter()
                    .find(|region| region.target == credit)
                    .unwrap()
                    .cells
                    .first()
                    .unwrap();
                let event = TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                    MouseEventKind::Down(MouseButton::Left),
                    point.0,
                    point.1,
                ));
                driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(event, true)],
                        true,
                    )
                    .await;
                assert_eq!(*calls.lock().unwrap(), vec!["credits"]);
                assert_eq!(
                    fixture.stack.snapshot().top().focus.current(),
                    Some(&credit)
                );
                assert_eq!(fixture.scroll.position().offset, offset);
                fixed_rows(driver.frames.last().unwrap());
            })
            .await
            .unwrap();
        }
    }
    #[tokio::test]
    async fn clearing_a_dynamic_status_restores_the_exact_content_sized_dialog_without_an_empty_gap()
     {
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(FixedSlotsScene(journal,width:76u16,height:30u16,rows:3u16,content_height:true));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let before = driver.frames.last().unwrap().clone();
            assert_eq!(before.round_frame_text().lines().count(), 9);
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::F(8)), true)],
                    true,
                )
                .await;
            let shown = driver.frames.last().unwrap();
            assert_eq!(shown.round_frame_text().lines().count(), 11);
            assert_eq!(
                row_of(shown, "Close"),
                row_of(shown, "Credit balance refreshed.") + 2
            );
            driver
                .drive(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::F(8)), true)],
                    true,
                )
                .await;
            assert_eq!(
                driver.frames.last().unwrap(),
                &before,
                "a cleared slot contributes no retained height or gap"
            );
        })
        .await
        .unwrap();
    }
}
