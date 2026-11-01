use crate::test_support::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Checked, Placement, ScopeId},
    controls::{
        fields::{MenuButton, SegmentedSwitch, button_target, segmented_targets},
        menu_overlay,
    },
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, GroupId, TargetId},
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{SurfaceId, SurfaceModel},
    },
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
    declared_modified_arrow: bool,
}
#[component]
fn SwitchScene(props: &SwitchSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let all = hooks.use_state(|| false);
    let stack = use_surface_stack(&mut hooks, |stack| {
        let folder = ActionDef {
            risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
            toolbar_label: None,
            id: action("folder"),
            title: "This folder".into(),
            keys: vec!["F2".parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: Some(Checked::new(move || !all.get())),
            placement: Placement::Menu { section: "Scope" },
            run: Handler::from(move |()| {
                let mut all = all;
                all.set(false);
            }),
        };
        let mut every = ActionDef {
            risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
            toolbar_label: None,
            id: action("all"),
            title: "All sessions".into(),
            keys: vec!["F3".parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: Some(Checked::new(move || all.get())),
            placement: Placement::Menu { section: "Scope" },
            run: Handler::from(move |()| {
                let mut all = all;
                all.set(true);
            }),
        };
        if props.declared_modified_arrow {
            every.keys.push("Ctrl+Left".parse().unwrap());
        }
        let menu = ActionDef {
            risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
            toolbar_label: None,
            id: action("menu"),
            title: "Menu".into(),
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
                            TargetId::for_action(&action("menu")),
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
            move || stack.viewport().0.saturating_sub(12),
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
    element!(InputOwner(stack,width:76u16,height:20u16,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,vec![u8::from(all.get())]),(SurfaceId::Screen,stack.snapshot().top().focus.current().unwrap().as_str().as_bytes().to_vec())]))) {OverlayHost {View {MenuButton(id:TargetId::for_action(&action("menu")),action:action("menu")) SegmentedSwitch(actions:vec![action("folder"),action("all")],available_width:stack.viewport().0.saturating_sub(12))}}})
}
fn click_label(driver: &EventDriver, label: &str, menu: bool) -> TerminalEvent {
    let grid = driver.frames.last().unwrap();
    let (rows, left, right) = if menu {
        let (top, left) = grid
            .cells
            .iter()
            .enumerate()
            .find_map(|(row, cells)| {
                cells
                    .iter()
                    .position(|cell| cell.text == "╭")
                    .map(|column| (row, column))
            })
            .unwrap();
        let right = grid.cells[top]
            .iter()
            .rposition(|cell| cell.text == "╮")
            .unwrap();
        let bottom = (top..grid.height)
            .find(|row| grid.cells[*row][left].text == "╰")
            .unwrap();
        (top + 1..bottom, left, right)
    } else {
        (0..1, 0, grid.width - 1)
    };
    let row = rows
        .into_iter()
        .find(|row| grid.row_text(*row).contains(label))
        .unwrap();
    let first = label.chars().next().unwrap().to_string();
    let column = grid.cells[row]
        .iter()
        .enumerate()
        .find(|(column, cell)| cell.text == first && *column >= left && *column <= right)
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
    let panel = grid.round_frame_text();
    let rows = panel.lines().collect::<Vec<_>>();
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
async fn modified_arrows_require_a_declared_action_and_plain_arrows_keep_group_navigation() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for declared in [false, true] {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(SwitchScene(journal,declared_modified_arrow:declared));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Tab), true)], true)
                    .await;
                let modified = [
                    (KeyCode::Left, KeyModifiers::CONTROL),
                    (KeyCode::Up, KeyModifiers::ALT),
                    (KeyCode::Right, KeyModifiers::SHIFT),
                ]
                .into_iter()
                .map(|(code, modifiers)| {
                    let mut key = KeyEvent::new(KeyEventKind::Press, code);
                    key.modifiers = modifiers;
                    (TerminalEvent::Key(key), false)
                })
                .collect();
                driver
                    .drive_checked(&mut frames, mode, modified, false, |ack| {
                        assert_eq!(
                            ack.snapshot.0[1].1, b"action:folder",
                            "modified arrows cannot fall through to group focus navigation"
                        );
                        assert_eq!(
                            ack.snapshot.0[0].1,
                            [u8::from(declared)],
                            "declared Ctrl+Left retains priority and runs its own action"
                        );
                    })
                    .await;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::Left), true)],
                        true,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[1].1, b"action:all",
                                "plain Left moves to the other grouped target"
                            );
                        },
                    )
                    .await;
                let mut reverse = KeyEvent::new(KeyEventKind::Press, KeyCode::Tab);
                reverse.modifiers = KeyModifiers::SHIFT;
                driver
                    .drive_checked(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::Key(reverse), true)],
                        true,
                        |ack| {
                            assert_eq!(
                                ack.snapshot.0[1].1, b"action:folder",
                                "Shift+Tab keeps reverse traversal"
                            );
                        },
                    )
                    .await;
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
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
