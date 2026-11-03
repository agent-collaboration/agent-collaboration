use crate::{
    controls::list::{ListGroup, ListGroupHeading, ListNavigationPolicy, ListRowKey, ListRows},
    test_support::{
        event_driver::{EventDriver, InputMode},
        grids::STANDARD_GRIDS,
        list_scene::{ListHandle, ListScene, key},
        recording_backend::recording_terminal_render_loop,
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

fn grouped_rows() -> ListRows {
    ListRows::Grouped(
        (0..10)
            .map(|group| ListGroup {
                heading: ListGroupHeading {
                    name: format!("Group {group}"),
                    count: 6,
                    summary: None,
                },
                keys: (group * 6..group * 6 + 6)
                    .map(|number| ListRowKey::new(format!("Record {number:02}")).unwrap())
                    .collect(),
            })
            .collect(),
    )
}
#[tokio::test]
async fn grouped_content_scroll_counts_headings_while_arrows_select_and_reveal_only_records() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for (width, height) in STANDARD_GRIDS {
            let handle = ListHandle::default();
            let selections = Arc::new(Mutex::new(Vec::new()));
            let (mut driver, config, journal) = EventDriver::new();
            let heights = (0..60)
                .map(|index| if index % 2 == 0 { 2 } else { 1 })
                .collect();
            let mut ui = element!(ListScene(journal,handle:handle.clone(),selections:selections.clone(),width:Some(width),height:Some(height),viewport_rows:Some(height-4),initial_rows:Some(grouped_rows()),row_heights:Some(heights),navigation_policy:ListNavigationPolicy::ContentScroll));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.get();
                let viewport = height - 4;
                let maximum = 100 - viewport;
                let position = fixture.control.scroll().position();
                assert_eq!(
                    position.natural_height, 100,
                    "sixty 2/1-row records plus ten measured headings"
                );
                assert_eq!(position.viewport_height, viewport);
                assert_eq!(position.maximum(), maximum);
                let steps = [
                    KeyCode::PageDown,
                    KeyCode::End,
                    KeyCode::PageUp,
                    KeyCode::Home,
                    KeyCode::End,
                ]
                .into_iter()
                .map(|code| (key(code), true))
                .collect();
                driver
                    .drive_checked(&mut frames, mode, steps, true, |ack| {
                        assert_eq!(
                            std::str::from_utf8(&ack.snapshot.0[0].1).unwrap(),
                            "Record 00|list",
                            "pages, bounds and wheel retain the selected row key"
                        );
                        let actual = std::str::from_utf8(&ack.snapshot.0[1].1)
                            .unwrap()
                            .parse::<u16>()
                            .unwrap();
                        assert_eq!(
                            actual,
                            [viewport, maximum, maximum - viewport, 0, maximum][ack.sequence - 1],
                            "content scroll counts real heading and record rows"
                        );
                    })
                    .await;
                for (kind, expected_offset) in [
                    (MouseEventKind::ScrollUp, maximum - 1),
                    (MouseEventKind::ScrollDown, maximum),
                ] {
                    let map =
                        fixture.stack.hit_regions().published().expect(
                            "the preceding changed frame publishes current pointer geometry",
                        );
                    let point = *map
                        .targets
                        .iter()
                        .find(|region| region.target == fixture.control.scroll_target())
                        .unwrap()
                        .cells
                        .first()
                        .unwrap();
                    driver
                        .drive_checked(
                            &mut frames,
                            mode,
                            vec![(
                                TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                    kind, point.0, point.1,
                                )),
                                true,
                            )],
                            true,
                            |ack| {
                                assert_eq!(
                                    std::str::from_utf8(&ack.snapshot.0[0].1).unwrap(),
                                    "Record 00|list",
                                    "pages, bounds and wheel retain the selected row key"
                                );
                                assert_eq!(
                                    std::str::from_utf8(&ack.snapshot.0[1].1)
                                        .unwrap()
                                        .parse::<u16>()
                                        .unwrap(),
                                    expected_offset,
                                    "wheel scrolls one content row without selecting"
                                );
                            },
                        )
                        .await;
                }
                assert!(selections.lock().unwrap().is_empty());
                let expected_down = (1..=6)
                    .map(|index| format!("Record {index:02}"))
                    .collect::<Vec<_>>();
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Down), true); 6],
                        true,
                        |ack| {
                            assert_eq!(
                                std::str::from_utf8(&ack.snapshot.0[0].1)
                                    .unwrap()
                                    .split('|')
                                    .next()
                                    .unwrap(),
                                expected_down[ack.sequence - 8],
                                "Down skips heading slots and chooses the next opaque row key"
                            );
                        },
                    )
                    .await;
                let frame = driver.frames.last().unwrap();
                assert!(
                    (1..usize::from(height - 3))
                        .any(|line| frame.row_text(line).contains("Group 1")),
                    "crossing a heading reveals it with its first record"
                );
                assert!(
                    (1..usize::from(height - 3))
                        .any(|line| frame.row_text(line).contains("Record 06")),
                    "the selected record is revealed after manual content scrolling"
                );
                let expected_up = (0..6)
                    .rev()
                    .map(|index| format!("Record {index:02}"))
                    .collect::<Vec<_>>();
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Up), true); 6],
                        true,
                        |ack| {
                            assert_eq!(
                                std::str::from_utf8(&ack.snapshot.0[0].1)
                                    .unwrap()
                                    .split('|')
                                    .next()
                                    .unwrap(),
                                expected_up[ack.sequence - 14],
                                "Up skips headings and selects by row key"
                            );
                        },
                    )
                    .await;
                assert_eq!(fixture.control.scroll().position().offset, 0);
                let frame = driver.frames.last().unwrap();
                assert!(frame.row_text(1).contains("Group 0"));
                assert!(frame.row_text(2).contains("Record 00"));
                let expected = expected_down
                    .into_iter()
                    .chain(expected_up)
                    .map(Some)
                    .collect::<Vec<_>>();
                assert_eq!(*selections.lock().unwrap(), expected);
            })
            .await
            .unwrap();
        }
    }
}
