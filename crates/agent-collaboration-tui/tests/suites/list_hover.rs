#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, InputMode},
        list_scene::{ListHandle, ListScene, key, target},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{controls::ListRowKey, surface_stack::SurfaceId};
    use iocraft::prelude::*;
    use std::sync::{Arc, Mutex};

    async fn hover_scenario(enabled: bool) {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let handle = ListHandle::default();
            let activated = Arc::new(Mutex::new(Vec::new()));
            let selections = Arc::new(Mutex::new(Vec::new()));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(ListScene(journal,handle:handle.clone(),activated:activated.clone(),selections:selections.clone(),hover_selection:enabled,marker_offset:2u16,viewport_rows:Some(4u16)));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let fixture = handle.get();
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Tab), true)], true)
                    .await;
                assert_eq!(
                    fixture.stack.snapshot().top().focus.current(),
                    Some(&target("toolbar"))
                );
                fixture.control.scroll().handle_event(&key(KeyCode::Down));
                driver.initial(&mut frames).await;
                assert_eq!(fixture.control.scroll().position().offset, 1);
                let bravo = ListRowKey::new("bravo").unwrap();
                let map = fixture.stack.hit_regions().published().unwrap();
                let region = map
                    .targets
                    .iter()
                    .find(|region| region.target == fixture.control.row_target(&bravo))
                    .unwrap();
                let visible_rows = region
                    .cells
                    .iter()
                    .map(|(_, row)| *row)
                    .collect::<std::collections::HashSet<_>>();
                assert_eq!(
                    visible_rows.len(),
                    2,
                    "Bravo is partially visible: revealing its full three-row height would move the viewport"
                );
                let point = *region.cells.first().unwrap();
                let moved = TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                    MouseEventKind::Moved,
                    point.0,
                    point.1,
                ));
                driver
                    .drive_checked(&mut frames, mode, vec![(moved, enabled)], enabled, |_| {
                        assert_eq!(
                            fixture.control.selected().unwrap().as_str(),
                            if enabled { "bravo" } else { "alpha" },
                            "hover selects only when opted in"
                        );
                        assert_eq!(
                            fixture.stack.snapshot().top().focus.current(),
                            Some(&target(if enabled { "list" } else { "toolbar" })),
                            "hover focuses the whole list only when opted in"
                        );
                        assert!(
                            activated.lock().unwrap().is_empty(),
                            "hover never activates"
                        );
                    })
                    .await;
                assert_eq!(
                    fixture.control.scroll().position().offset,
                    1,
                    "after completed hover draw, selection must not reveal, scroll or re-window"
                );
                assert_eq!(
                    *selections.lock().unwrap(),
                    if enabled {
                        vec![Some("bravo".to_owned())]
                    } else {
                        Vec::new()
                    }
                );
                if enabled {
                    let map = fixture.stack.hit_regions().published().unwrap();
                    let alpha = ListRowKey::new("alpha").unwrap();
                    let point = *map
                        .targets
                        .iter()
                        .find(|region| region.target == fixture.control.row_target(&alpha))
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
                                    MouseEventKind::Down(MouseButton::Left),
                                    point.0,
                                    point.1,
                                )),
                                true,
                            )],
                            true,
                            |_| {
                                assert_eq!(fixture.control.selected().unwrap().as_str(), "alpha");
                                assert!(
                                    activated.lock().unwrap().is_empty(),
                                    "row click is selection only"
                                );
                            },
                        )
                        .await;
                    let acknowledgements = driver
                        .drive(&mut frames, mode, vec![(key(KeyCode::Enter), true)], true)
                        .await;
                    assert_eq!(acknowledgements[0].snapshot.0[0].0, SurfaceId::Screen);
                    assert_eq!(
                        *activated.lock().unwrap(),
                        vec!["alpha"],
                        "Enter activates the clicked row exactly once"
                    );
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
    #[tokio::test]
    async fn opted_in_list_hover_selects_without_activation_or_revealing_partial_rows() {
        hover_scenario(true).await;
    }
    #[tokio::test]
    async fn list_hover_selection_is_off_by_default() {
        hover_scenario(false).await;
    }
}
