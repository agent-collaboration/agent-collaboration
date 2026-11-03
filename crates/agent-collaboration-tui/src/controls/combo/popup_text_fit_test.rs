use crate::test_support::{
    dialog_scene::DialogScene,
    event_driver::{EventDriver, InputMode},
    frame_grid::FrameGrid,
    list_scene::key,
    popup_text_scene::{LONG_REASON, PopupTextScene, REASON, field},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    actions::Availability,
    controls::list::{OptionKey, SelectOption},
    surfaces::{surface_handle::SurfaceStackHandle, surface_stack::SurfaceId},
};
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
use unicode_width::UnicodeWidthStr;

fn panel_bounds(frame: &FrameGrid) -> (usize, usize, usize) {
    let (left, top) = frame
        .cells
        .iter()
        .enumerate()
        .find_map(|(row, cells)| {
            cells
                .iter()
                .position(|cell| cell.text == "╭")
                .map(|column| (column, row))
        })
        .unwrap();
    let right = frame.cells[top]
        .iter()
        .rposition(|cell| cell.text == "╮")
        .unwrap();
    (left, top, right)
}
fn surface_width(stack: SurfaceStackHandle, surface: SurfaceId) -> u16 {
    let map = stack.hit_regions().published().unwrap();
    let cells = &map
        .surfaces
        .iter()
        .find(|region| region.surface == surface)
        .unwrap()
        .cells;
    cells.iter().map(|cell| cell.0).max().unwrap() - cells.iter().map(|cell| cell.0).min().unwrap()
        + 1
}
async fn assert_late_option_width(suspended: bool) {
    // Arrange a real scoped control; Help must retain this same popup model.
    let stack_out = Arc::new(Mutex::new(None));
    let control_out = Arc::new(Mutex::new(None));
    let data_out = Arc::new(Mutex::new(None));
    let help_out = Arc::new(Mutex::new(None));
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(DialogScene(journal,form:true,width:120u16,height:36u16,surface_stack:stack_out.clone(),combo:control_out.clone(),data:data_out.clone(),help_control:help_out.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![
                    (key(KeyCode::Enter), true),
                    (TerminalEvent::BracketedPaste("remote".into()), true),
                ],
                true,
            )
            .await;
        let stack = stack_out.lock().unwrap().unwrap();
        let control = control_out.lock().unwrap().clone().unwrap();
        let data = data_out.lock().unwrap().unwrap();
        let help = help_out.lock().unwrap().clone().unwrap();
        let popup = stack.snapshot().top_id();
        let initial_width = surface_width(stack, popup);
        if suspended {
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::F(1)), true)],
                    true,
                    |_| {
                        assert!(
                            help.is_active(stack.snapshot().top_id()),
                            "F1 opens Help before waiting for its redraw"
                        );
                    },
                )
                .await;
            assert_ne!(stack.snapshot().top_id(), popup);
        }
        // Act: a provider update changes only the refusal's text width.
        let mut unavailable =
            SelectOption::available(OptionKey::new("beta").unwrap(), "Beta remote");
        unavailable.availability = Availability::Unavailable {
            reason: LONG_REASON.to_owned(),
        };
        assert!(control.replace_options(vec![unavailable]).unwrap());
        if suspended {
            driver
                .drive_checked(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(key(KeyCode::Esc), true)],
                    true,
                    |_| {
                        assert!(!help.is_active(stack.snapshot().top_id()));
                        assert_eq!(
                            stack.snapshot().top_id(),
                            popup,
                            "Esc restores the retained popup before waiting for its redraw"
                        );
                    },
                )
                .await;
        } else {
            driver.initial(&mut frames).await;
        }
        // Assert native dimensions and painted text, preserving identity/query/value.
        assert_eq!(stack.snapshot().top_id(), popup);
        assert!(usize::from(surface_width(stack, popup)) >= LONG_REASON.width() + 5);
        let expanded = driver.frames.last().unwrap();
        assert!((0..expanded.height).any(|row| expanded.row_text(row).contains(LONG_REASON)));
        assert_eq!(data.client.read().query().text(), "remote");
        assert_eq!(
            data.client.read().value(),
            Some(&OptionKey::new("beta").unwrap())
        );
        assert!(
            control
                .replace_options(vec![SelectOption::available(
                    OptionKey::new("beta").unwrap(),
                    "Beta remote"
                )])
                .unwrap()
        );
        driver.initial(&mut frames).await;
        assert_eq!(
            surface_width(stack, popup),
            initial_width,
            "removing the widest painted text shrinks the continuing popup"
        );
        assert_eq!(stack.snapshot().top_id(), popup);
        assert_eq!(data.client.read().query().text(), "remote");
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn live_options_resize_the_continuing_popup_in_both_directions() {
    assert_late_option_width(false).await;
}
#[tokio::test]
async fn late_options_resize_a_help_suspended_popup_on_return() {
    assert_late_option_width(true).await;
}
async fn assert_query(width: u16, height: u16, query: &str, reason: bool) {
    let (mut driver, config, journal) = EventDriver::new();
    let handle = Arc::new(Mutex::new(None));
    let raw = Arc::new(Mutex::new(Vec::new()));
    let captured = raw.clone();
    let mut ui = element!(PopupTextScene(journal,width,height,reason,handle:handle.clone()));
    let mut frames = Box::pin(
        recording_terminal_render_loop(&mut ui, config)
            .inspect(move |canvas| captured.lock().unwrap().push(canvas.clone())),
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(key(KeyCode::F(3)), true)],
                true,
            )
            .await;
        let start = raw.lock().unwrap().len();
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(TerminalEvent::BracketedPaste(query.to_owned()), true)],
                true,
            )
            .await;
        let first = FrameGrid::from_canvas(raw.lock().unwrap().get(start).unwrap()).unwrap();
        eprintln!(
            "QUERY FIRST {width}x{height} {query}:\n{}",
            first.round_frame_text()
        );
        assert!(
            (0..first.height).any(|row| first.row_text(row).contains(&format!("Filter: {query}"))),
            "the first query frame must keep the whole fitting prefix/query: {}",
            first.round_frame_text()
        );
        let fixture = handle.lock().unwrap().clone().unwrap();
        assert_eq!(fixture.value.read().query().text(), query);
        assert_eq!(
            fixture.value.read().value(),
            Some(&OptionKey::new("all").unwrap())
        );
        let (left, top, right) = panel_bounds(&first);
        let width = right - left + 1;
        assert!(width >= query.width() + 11, "filter8+query+cursor1+border2");
        if fixture.value.read().matching().is_empty() {
            assert!((0..first.height).any(|row| first.row_text(row).contains("No match")));
        }
        if reason {
            let body = (top + 1..first.height)
                .map(|row| {
                    first.cells[row][left + 1..right]
                        .iter()
                        .map(|cell| cell.text.as_str())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join(" ");
            let words = body.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(
                words.contains(REASON),
                "full refusal must fit the clamped popup: {words}"
            );
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn source_query_32x24_keeps_its_complete_attributable_filter() {
    assert_query(32, 24, "Interactive", false).await;
}
#[tokio::test]
async fn source_query_48x12_keeps_its_complete_filter_and_external_refusal() {
    assert_query(48, 12, "Interactive", true).await;
}
#[tokio::test]
async fn no_match_32x24_keeps_the_complete_route_query() {
    assert_query(32, 24, "zz-route-absent", false).await;
}
#[tokio::test]
async fn no_match_48x12_keeps_the_complete_route_query() {
    assert_query(48, 12, "zz-route-absent", false).await;
}
#[tokio::test]
async fn refusal_width_can_use_the_terminal_beyond_the_former_64_cell_cap() {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui =
        element!(PopupTextScene(journal,width:120u16,height:36u16,reason:true,long_reason:true));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(key(KeyCode::F(3)), true)],
                true,
            )
            .await;
        let frame = driver.frames.last().unwrap();
        let (left, _, right) = panel_bounds(frame);
        assert!(
            right - left + 1 >= LONG_REASON.width() + 5,
            "widest painted refusal sets the preference: {}",
            frame.round_frame_text()
        );
        assert!(frame.round_frame_text().contains(LONG_REASON));
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn an_overlong_query_scrolls_to_its_visible_cursor_after_terminal_clamping() {
    let (mut driver, config, journal) = EventDriver::new();
    let raw = Arc::new(Mutex::new(Vec::new()));
    let captured = raw.clone();
    let mut ui = element!(PopupTextScene(journal,width:32u16,height:24u16));
    let mut frames = Box::pin(
        recording_terminal_render_loop(&mut ui, config)
            .inspect(move |canvas| captured.lock().unwrap().push(canvas.clone())),
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(key(KeyCode::F(3)), true)],
                true,
            )
            .await;
        let start = raw.lock().unwrap().len();
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(
                    TerminalEvent::BracketedPaste(
                        "before-abcdefghijklmnopqrstuvwxyz-終".to_owned(),
                    ),
                    true,
                )],
                true,
            )
            .await;
        let first = FrameGrid::from_canvas(raw.lock().unwrap().get(start).unwrap()).unwrap();
        let (left, top, right) = panel_bounds(&first);
        eprintln!("CLAMPED QUERY FIRST:\n{}", first.round_frame_text());
        assert_eq!(
            right - left + 1,
            32,
            "use all available terminal cells before scrolling the editor"
        );
        assert!(
            first.cell(right - 1, top + 1).unwrap().style.underline,
            "the end cursor remains visible in the first clamped frame"
        );
        assert!(first.row_text(top + 1).contains("終"));
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn opening_an_offscreen_destination_reveals_it_before_anchoring_at_48x12() {
    let (mut driver, config, journal) = EventDriver::new();
    let handle = Arc::new(Mutex::new(None));
    let mut ui = element!(PopupTextScene(journal,width:48u16,height:12u16,scrolled:true,handle:handle.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
            driver.initial(&mut frames).await;let fixture=handle.lock().unwrap().clone().unwrap();
            let map=fixture.stack.hit_regions().published().unwrap();
            assert!(map.targets.iter().find(|region|region.surface==fixture.parent&&region.target==field()).unwrap().cells.is_empty());
            let focus=fixture.stack.snapshot().top().focus.current().cloned();
            assert_eq!(fixture.scroll.position().offset,0);
            driver.drive(&mut frames,InputMode::FrameSeparated,vec![(key(KeyCode::F(3)),true)],true).await;
            assert!(fixture.scroll.position().offset>0,"opening must reveal the parent's unpainted field, never silently leave an invisible dropdown");
            let map=fixture.stack.hit_regions().published().unwrap();
            let anchor=map.targets.iter().find(|region|region.surface==fixture.parent&&region.target==field()).unwrap();
            assert!(!anchor.cells.is_empty());
            assert!((0..driver.frames.last().unwrap().height).any(|row|driver.frames.last().unwrap().row_text(row).contains("Filter:")));
            assert_ne!(fixture.stack.snapshot().top_id(),fixture.parent);
            driver.drive(&mut frames,InputMode::FrameSeparated,vec![(TerminalEvent::BracketedPaste("zz-route-absent".to_owned()),true)],true).await;
            assert!((0..driver.frames.last().unwrap().height).any(|row|driver.frames.last().unwrap().row_text(row).contains("Filter: zz-route-absent")));
            driver.drive(&mut frames,InputMode::FrameSeparated,vec![(key(KeyCode::Esc),true)],true).await;
            assert_eq!(fixture.stack.snapshot().top_id(),fixture.parent);
            assert_eq!(fixture.stack.snapshot().top().focus.current(),focus.as_ref());
            assert!((0..driver.frames.last().unwrap().height).any(|row|driver.frames.last().unwrap().row_text(row).contains("Destination")));
            assert!(matches!(fixture.parent,SurfaceId::Overlay(_)));
            // Manual scrolling must not make the same measured field's next open inert.
            driver.drive(&mut frames,InputMode::FrameSeparated,vec![(key(KeyCode::Home),true)],true).await;
            assert_eq!(fixture.scroll.position().offset,0);
            let map=fixture.stack.hit_regions().published().unwrap();
            assert!(map.targets.iter().find(|region|region.surface==fixture.parent&&region.target==field()).unwrap().cells.is_empty());
            driver.drive(&mut frames,InputMode::FrameSeparated,vec![(key(KeyCode::F(3)),true)],true).await;
            assert!(fixture.scroll.position().offset>0,"every explicit opening must reveal the field again after manual scroll");
            assert!((0..driver.frames.last().unwrap().height).any(|row|driver.frames.last().unwrap().row_text(row).contains("Filter:")));
        }).await.unwrap();
}
#[tokio::test]
#[should_panic(expected = "destination-field requires parent_scroll")]
async fn an_offscreen_field_without_parent_scroll_names_the_binding_error() {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(PopupTextScene(journal,width:48u16,height:12u16,scrolled:true,omit_parent_scroll:true));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                InputMode::FrameSeparated,
                vec![(key(KeyCode::F(3)), false)],
                false,
            )
            .await;
    })
    .await
    .unwrap();
}
