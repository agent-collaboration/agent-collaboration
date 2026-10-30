#[cfg(test)]
mod tests {
    use crate::support::{
        event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
        recording_backend::recording_terminal_render_loop,
    };
    use agent_collaboration_tui::{
        actions::{
            ActionDef, ActionId, ActionRegistry, AvailabilityFn, Checked, Placement, ScopeId,
        },
        controls::{MenuButton, SegmentedSwitch, button_target, menu_overlay, segmented_targets},
        focus_model::{FocusModel, GroupId, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{OverlayLayout, SurfaceId, SurfaceModel},
    };
    use iocraft::prelude::*;
    fn action(name: &str) -> ActionId {
        ActionId::new(name).unwrap()
    }
    fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
    }
    #[derive(Props)]
    struct SwitchSceneProps {
        #[iocraft(required)]
        journal: EventJournal,
    }
    #[component]
    fn SwitchScene(props: &SwitchSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        record_draw(&mut hooks, &props.journal);
        let all = hooks.use_state(|| false);
        let stack = use_surface_stack(&mut hooks, |stack| {
            let folder = ActionDef {
                id: action("folder"),
                title: "This folder",
                keys: vec!["F2".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: Some(Checked::new(move || !all.get())),
                placement: Placement::Menu { section: "Scope" },
                run: Handler::from(move |()| {
                    let mut all = all;
                    all.set(false);
                }),
            };
            let every = ActionDef {
                id: action("all"),
                title: "All sessions",
                keys: vec!["F3".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: Some(Checked::new(move || all.get())),
                placement: Placement::Menu { section: "Scope" },
                run: Handler::from(move |()| {
                    let mut all = all;
                    all.set(true);
                }),
            };
            let menu = ActionDef {
                id: action("menu"),
                title: "Menu",
                keys: vec!["F10".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: Handler::from(move |()| {
                    let snapshot = stack.snapshot();
                    stack
                        .open_overlay(
                            menu_overlay(
                                stack,
                                &snapshot.top().actions,
                                OverlayLayout::AtCells {
                                    left: 10,
                                    top: 3,
                                    width: 36,
                                    height: 7,
                                },
                            )
                            .unwrap(),
                        )
                        .unwrap();
                }),
            };
            let mut targets = vec![button_target(TargetId::for_action(&menu.id), &menu)];
            targets.extend(segmented_targets(
                &[folder.clone(), every.clone()],
                GroupId::new("scope").unwrap(),
            ));
            SurfaceModel {
                focus: FocusModel::new(targets).unwrap(),
                actions: ActionRegistry::new(
                    ScopeId::new("switch").unwrap(),
                    vec![menu, folder, every],
                )
                .unwrap(),
                primary_action: None,
            }
        });
        let journal = props.journal.clone();
        element!(InputOwner(stack,width:76u16,height:20u16,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,vec![u8::from(all.get())])]))) {OverlayHost {View {MenuButton(id:TargetId::for_action(&action("menu")),action:action("menu")) SegmentedSwitch(actions:vec![action("folder"),action("all")])}}})
    }
    fn click_label(driver: &EventDriver, label: &str, menu: bool) -> TerminalEvent {
        let grid = driver.frames.last().unwrap();
        let rows = if menu { 4..9 } else { 0..1 };
        let row = rows
            .into_iter()
            .find(|row| grid.row_text(*row).contains(label))
            .unwrap();
        let first = label.chars().next().unwrap().to_string();
        let column = grid.cells[row]
            .iter()
            .enumerate()
            .find(|(column, cell)| cell.text == first && (!menu || *column >= 10))
            .unwrap()
            .0;
        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            u16::try_from(column).unwrap(),
            u16::try_from(row).unwrap(),
        ))
    }
    fn checked_switch(driver: &EventDriver, label: &str) {
        let grid = driver.frames.last().unwrap();
        let text = grid.row_text(0);
        assert_eq!(text.matches('●').count(), 1);
        assert!(text.contains(&format!("● {label}")), "{text}");
    }
    fn checked_menu(driver: &EventDriver, label: &str) {
        let grid = driver.frames.last().unwrap();
        let rows = (4..9)
            .map(|row| {
                grid.cells[row][10..46]
                    .iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rows.iter()
                .map(|row| row.matches('✓').count())
                .sum::<usize>(),
            1
        );
        assert!(
            rows.iter()
                .any(|row| row.contains('✓') && row.contains(label)),
            "{rows:?}"
        );
    }
    #[tokio::test]
    async fn switch_selection_and_menu_checks_follow_pointer_menu_and_key_effects() {
        for mode in [InputMode::Burst, InputMode::FrameSeparated] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(SwitchScene(journal));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                checked_switch(&driver, "This folder");
                let click = click_label(&driver, "All sessions", false);
                driver
                    .drive_checked(&mut frames, mode, vec![(click, true)], true, |ack| {
                        assert_eq!(ack.snapshot.0[0].1, [1])
                    })
                    .await;
                checked_switch(&driver, "All sessions");
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(10)), true)], true)
                    .await;
                checked_menu(&driver, "All sessions");
                let click = click_label(&driver, "This folder", true);
                driver
                    .drive_checked(&mut frames, mode, vec![(click, true)], true, |ack| {
                        assert_eq!(ack.snapshot.0[0].1, [0])
                    })
                    .await;
                checked_switch(&driver, "This folder");
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::F(10)), true)], true)
                    .await;
                checked_menu(&driver, "This folder");
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::F(3)), true)],
                        true,
                        |ack| assert_eq!(ack.snapshot.0[0].1, [1]),
                    )
                    .await;
                checked_switch(&driver, "All sessions");
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::F(2)), true)],
                        true,
                        |ack| assert_eq!(ack.snapshot.0[0].1, [0]),
                    )
                    .await;
                checked_switch(&driver, "This folder");
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
