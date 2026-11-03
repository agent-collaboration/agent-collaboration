use crate::dialog::help::{HelpContent, HelpInputConvention};
use crate::{
    test_support::help_scene::{HelpHandle, HelpScene, key, text},
    test_support::{
        event_driver::{EventDriver, InputMode},
        recording_backend::recording_terminal_render_loop,
    },
};
use iocraft::prelude::*;
#[tokio::test]
async fn option_pages_and_bounds_scroll_generated_help_without_changing_its_scope() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let handle = HelpHandle::default();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HelpScene(journal,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            driver
                .drive(&mut frames, mode, vec![(key(KeyCode::F(1)), true)], true)
                .await;
            let fixture = handle.0.lock().unwrap().clone().unwrap();
            let surface = fixture.stack.snapshot().top_id();
            let scroll = fixture.control.scroll(surface).unwrap();
            let initial = scroll.position();
            assert!(
                initial.maximum() > initial.viewport_height,
                "generated Help has more than one page"
            );
            for (code, modifiers, expected) in [
                (KeyCode::Down, KeyModifiers::ALT, initial.viewport_height),
                (
                    KeyCode::Down,
                    KeyModifiers::ALT | KeyModifiers::SHIFT,
                    initial.maximum(),
                ),
                (
                    KeyCode::Up,
                    KeyModifiers::ALT,
                    initial.maximum().saturating_sub(initial.viewport_height),
                ),
                (KeyCode::Up, KeyModifiers::ALT | KeyModifiers::SHIFT, 0),
            ] {
                let mut event = KeyEvent::new(KeyEventKind::Press, code);
                event.modifiers = modifiers;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::Key(event), true)],
                        true,
                        |_| {
                            assert_eq!(fixture.stack.snapshot().top_id(), surface);
                            assert!(fixture.control.is_active(surface));
                            assert_eq!(
                                scroll.position().offset,
                                expected,
                                "Option aliases scroll generated Help by page/bound"
                            );
                        },
                    )
                    .await;
                if expected == initial.maximum() {
                    let frame = text(&driver);
                    assert!(
                        frame.contains("PgUp/PgDn · Option+↑/↓"),
                        "generated Help paints page aliases"
                    );
                    assert!(
                        frame.contains("Home/End · Option+Shift+↑/↓"),
                        "generated Help paints bounds aliases"
                    );
                }
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn help_end_and_last_reach_the_final_painted_line_and_scrollbar_end_at_every_grid() {
    assert_help_painted_end(true).await;
}

#[tokio::test]
async fn help_down_cannot_advance_beyond_the_independently_painted_final_line() {
    assert_help_painted_end(false).await;
}

async fn assert_help_painted_end(check_thumb: bool) {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
            let handle = HelpHandle::default();
            let (mut driver, config, journal) = EventDriver::new();
            let mut content = HelpContent::default();
            content.conventions.push(HelpInputConvention {
                key: "Final".into(),
                description: "HELP-END".into(),
            });
            let mut ui = element!(HelpScene(journal,handle:handle.clone(),content,width:Some(width),height:Some(height)));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(1)), true)], true)
                    .await;
                let initial = driver.frames.last().unwrap();
                let (top,left)=initial.cells.iter().enumerate().find_map(|(row,cells)|cells.iter().position(|cell|cell.text=="╭").map(|column|(row,column))).unwrap();
                let right=initial.cells[top].iter().position(|cell|cell.text=="╮").unwrap();
                let bottom=(top+1..initial.height).find(|row|initial.cell(left,*row).unwrap().text=="╰").unwrap();
                let expected = [
                    ((120,36),(16,8,88,21)),
                    ((76,30),(8,6,60,18)),
                    ((48,12),(3,2,42,8)),
                    ((32,24),(0,5,32,14)),
                ].into_iter().find_map(|(grid,rectangle)|(grid==(width,height)).then_some(rectangle)).expect("standard grid has a literal centered rectangle");
                assert_eq!((left,top,right-left+1,bottom-top+1),expected,"whole-cell origins preserve the existing centered rectangle");
                let fixture = handle.0.lock().unwrap().clone().unwrap();
                let scroll = fixture
                    .control
                    .scroll(fixture.stack.snapshot().top_id())
                    .unwrap();
                for (code, modifiers) in [
                    (KeyCode::End, KeyModifiers::NONE),
                    (KeyCode::Down, KeyModifiers::ALT | KeyModifiers::SHIFT),
                ] {
                    if scroll.position().offset > 0 {
                        driver
                            .drive(&mut frames, mode, vec![(key(KeyCode::Home), true)], true)
                            .await;
                    }
                    let mut event = KeyEvent::new(KeyEventKind::Press, code);
                    event.modifiers = modifiers;
                    driver
                        .drive(
                            &mut frames,
                            mode,
                            vec![(TerminalEvent::Key(event), true)],
                            true,
                        )
                        .await;
                    let position = scroll.position();
                    let frame = driver.frames.last().unwrap();
                    let last = usize::try_from(position.viewport_top).unwrap()
                        + usize::from(position.viewport_height)
                        - 1;
                    let line = frame.row_text(last);
                    assert!(
                        line.contains("HELP-END"),
                        "End/Last reaches the independently declared final painted Help line at {width}x{height} {mode:?}: {position:?}, {line}"
                    );
                    if !check_thumb {
                        driver
                            .drive_checked(
                                &mut frames,
                                mode,
                                vec![(key(KeyCode::Down), false)],
                                false,
                                |_|assert_eq!(scroll.position().offset,position.offset,"Down cannot expose a phantom row beyond the independently painted HELP-END"),
                            )
                            .await;
                        continue;
                    }
                    let thumb = frame.cells[last]
                        .iter()
                        .filter(|cell| cell.text == "█")
                        .count();
                    assert_eq!(
                        thumb, 1,
                        "Help thumb reaches the end in the same frame as its final content line at {width}x{height} {mode:?}: {position:?}\n{}",
                        (0..frame.height).map(|row|frame.row_text(row)).collect::<Vec<_>>().join("\n")
                    );
                }
            })
            .await
            .unwrap();
        }
    }
}
