use crate::{
    controls::list::{ListGroup, ListGroupHeading, ListNavigationPolicy, ListRowKey, ListRows},
    test_support::{
        event_driver::{EventAck, EventDriver, InputMode},
        grids::STANDARD_GRIDS,
        list_scene::{ListHandle, ListScene, key},
        recording_backend::recording_terminal_render_loop,
    },
};
use iocraft::prelude::*;

fn row_key(name: &str) -> ListRowKey {
    ListRowKey::new(name).unwrap()
}
fn grouped_rows() -> ListRows {
    ListRows::Grouped(
        (0..10)
            .map(|index| ListGroup {
                heading: ListGroupHeading {
                    name: if index == 0 {
                        "You".into()
                    } else {
                        format!("Group {index}")
                    },
                    count: 2,
                    summary: None,
                },
                keys: [index * 2, index * 2 + 1]
                    .map(|number| row_key(&format!("Record {number:02}")))
                    .into(),
            })
            .collect(),
    )
}
fn selection(ack: &EventAck) -> &str {
    std::str::from_utf8(&ack.snapshot.0[0].1)
        .unwrap()
        .split('|')
        .next()
        .unwrap()
}
fn offset(ack: &EventAck) -> u16 {
    std::str::from_utf8(&ack.snapshot.0[1].1)
        .unwrap()
        .parse()
        .unwrap()
}
async fn assert_heading_reveal(grids: &[(u16, u16)], displace_first: bool) {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for &(width, height) in grids {
            let handle = ListHandle::default();
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ListScene(journal,handle:handle.clone(),width:Some(width),height:Some(height),viewport_rows:Some(height-4),row_heights:Some(vec![2;20]),initial_rows:Some(grouped_rows())));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.get();
                if displace_first {
                    assert_eq!(fixture.control.scroll().handle_event(&key(KeyCode::End)), crate::surfaces::focus_model::InputUse::Scrolled);
                    driver.initial(&mut frames).await;
                    assert!(fixture.control.scroll().position().offset > 0);
                    assert_eq!(fixture.control.selected(), Some(row_key("Record 00")));
                } else {
                    assert_eq!(
                        fixture.control.scroll().position().offset,
                        0,
                        "the first record reveals its preceding heading when both fit"
                    );
                    driver.drive(&mut frames, mode, vec![(key(KeyCode::End), true)], true).await;
                    assert_eq!(fixture.control.selected(), Some(row_key("Record 19")));
                }
                driver.drive_checked(&mut frames, mode, vec![(key(KeyCode::Home), true)], true, |ack| {
                    assert_eq!(selection(ack), "Record 00");
                    assert_eq!(offset(ack), 0, "Home reveals the first heading even if its record was already selected");
                }).await;
                let frame = driver.frames.last().unwrap();
                let grid = (0..frame.height).map(|line| frame.row_text(line)).collect::<Vec<_>>().join("\n");
                assert!(frame.row_text(1).contains("You") && frame.row_text(1).contains("(2)"), "native {width}x{height} Home keeps the heading visible:\n{grid}");
                assert!(frame.row_text(2).contains("Record 00"), "the first record stays visible with its heading:\n{grid}");
                let target = fixture.control.row_target(&row_key("Record 00"));
                let map = fixture.stack.hit_regions().published().unwrap();
                let cells = map.targets.iter().find(|region| region.target == target).unwrap();
                assert!(!cells.cells.is_empty());
                assert!(cells.cells.iter().all(|(_, row)| *row != 1), "co-reveal never makes heading cells part of the record hit target");
                driver.drive_checked(&mut frames, mode, vec![(key(KeyCode::Down), true), (key(KeyCode::Down), true)], true, |ack| {
                    assert_eq!(selection(ack), if ack.sequence == 2 + usize::from(!displace_first) { "Record 01" } else { "Record 02" });
                }).await;
                let frame = driver.frames.last().unwrap();
                assert!((1..usize::from(height-3)).any(|line| frame.row_text(line).contains("Group 1")), "another group's first record reveals its heading");
                assert!((1..usize::from(height-3)).any(|line| frame.row_text(line).contains("Record 02")));
            }).await.unwrap();
        }
    }
}

#[tokio::test]
async fn first_record_and_home_reveal_the_heading_when_both_fit() {
    assert_heading_reveal(&STANDARD_GRIDS, false).await;
}
#[tokio::test]
async fn home_reveals_an_already_selected_first_heading_after_manual_scroll() {
    assert_heading_reveal(&STANDARD_GRIDS, true).await;
}
#[tokio::test]
async fn phone_home_keeps_the_first_group_heading_above_its_record() {
    assert_heading_reveal(&[(48, 12)], true).await;
}
#[tokio::test]
async fn wrapped_heading_and_first_record_are_revealed_together_when_the_pair_fits() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for (width, height) in STANDARD_GRIDS {
            let handle = ListHandle::default();
            let (mut driver, config, journal) = EventDriver::new();
            let mut rows = grouped_rows();
            if let ListRows::Grouped(groups) = &mut rows {
                groups[0].heading.name = "Wrapped heading ".repeat(9);
            }
            let mut ui = element!(ListScene(journal,handle:handle.clone(),width:Some(width),height:Some(height),viewport_rows:Some(height-4),row_heights:Some(vec![2;20]),initial_rows:Some(rows),navigation_policy:ListNavigationPolicy::ContentScroll));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                let fixture = handle.get();
                let position = fixture.control.scroll().position();
                // Forty record rows and nine other single-row headings.
                let heading_height = position.natural_height.checked_sub(49).unwrap();
                assert!(
                    heading_height > 1,
                    "the fixture must measure a wrapped heading"
                );
                assert!(
                    heading_height + 2 <= position.viewport_height,
                    "the measured heading and two-row record must fit together"
                );
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::End), true)], true)
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Home), true)],
                        true,
                        |ack| {
                            assert_eq!(selection(ack), "Record 00");
                            assert_eq!(
                                offset(ack),
                                0,
                                "reveal includes the complete measured wrapped heading"
                            );
                        },
                    )
                    .await;
                let frame = driver.frames.last().unwrap();
                assert!(frame.row_text(1).contains("Wrapped heading"));
                assert!(
                    frame
                        .row_text(usize::from(heading_height) + 1)
                        .contains("Record 00")
                );
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Down), true), (key(KeyCode::End), true)],
                        true,
                    )
                    .await;
                assert_eq!(fixture.control.selected(), Some(row_key("Record 01")));
                assert!(fixture.control.scroll().position().offset > 0);
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Up), true)],
                        true,
                        |ack| {
                            assert_eq!(selection(ack), "Record 00");
                            assert_eq!(
                                offset(ack),
                                0,
                                "reveal includes the complete measured wrapped heading"
                            );
                        },
                    )
                    .await;
                let frame = driver.frames.last().unwrap();
                assert!(frame.row_text(1).contains("Wrapped heading"));
                assert!(
                    frame
                        .row_text(usize::from(heading_height) + 1)
                        .contains("Record 00")
                );
            })
            .await
            .unwrap();
        }
    }
}
#[tokio::test]
async fn a_wrapped_heading_yields_to_the_record_when_the_pair_does_not_fit() {
    for (width, height) in STANDARD_GRIDS {
        let handle = ListHandle::default();
        let (mut driver, config, journal) = EventDriver::new();
        let rows = ListRows::Grouped(vec![ListGroup {
            heading: ListGroupHeading {
                name: "A deliberately long heading that wraps across several terminal rows "
                    .repeat(5),
                count: 2,
                summary: None,
            },
            keys: vec![row_key("alpha"), row_key("bravo")],
        }]);
        let mut ui = element!(ListScene(journal,handle:handle.clone(),width:Some(width),height:Some(height),viewport_rows:Some(1),initial_rows:Some(rows)));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let fixture = handle.get();
            assert_eq!(fixture.control.scroll().position().viewport_height, 1);
            assert!(
                fixture.control.scroll().position().offset > 0,
                "the oversized heading is scrolled away in favor of its record"
            );
            assert_eq!(fixture.control.selected(), Some(row_key("alpha")));
            assert!(
                driver.frames.last().unwrap().row_text(1).contains("alpha"),
                "the record wins when its wrapped heading cannot fit beside it in the viewport"
            );
        })
        .await
        .unwrap();
    }
}
