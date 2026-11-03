use crate::{
    test_support::help_scene::{HelpFixture, HelpHandle, HelpScene, TITLES, action, key, text},
    test_support::{
        event_driver::{EventDriver, InputMode},
        recording_backend::recording_terminal_render_loop,
    },
};
use agent_collaboration_tui::{controls::menu_overlay, surfaces::focus_model::TargetId};
use iocraft::prelude::*;
#[tokio::test]
async fn main_help_projects_bound_pairs_updates_availability_and_scrolls_by_keys_and_wheel() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        let handle = HelpHandle::default();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HelpScene(journal,handle:handle.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        let scenario = async {
            driver.initial(&mut frames).await;
            let opened = driver
                .drive(&mut frames, mode, vec![(key(KeyCode::F(1)), true)], true)
                .await;
            assert_eq!(opened.last().unwrap().snapshot.0.len(), 2);
            let top = text(&driver);
            assert!(
                top.lines()
                    .any(|line| line.contains("F1") && line.contains("Help"))
            );
            assert!(
                top.lines()
                    .any(|line| line.contains("F8") && line.contains("Execute"))
            );
            assert!(
                top.lines()
                    .any(|line| line.contains("Ctrl+Alt+A") && line.contains(TITLES[0])),
                "{top}"
            );
            let HelpFixture {
                control: help,
                mut enabled,
                ..
            } = handle.0.lock().unwrap().as_ref().unwrap().clone();
            let surface = opened.last().unwrap().snapshot.0[1].0;
            let scroll = help.scroll(surface).unwrap();
            enabled.set(false);
            driver.initial(&mut frames).await;
            assert!(text(&driver).lines().any(|line| line.contains("F8")
                && line.contains("Execute")
                && line.contains("Already executed")));
            let mut seen = TITLES
                .into_iter()
                .filter(|title| text(&driver).contains(title))
                .collect::<Vec<_>>();
            for _ in 0..scroll
                .position()
                .maximum()
                .div_ceil(scroll.position().viewport_height.max(1))
            {
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(key(KeyCode::PageDown), true)],
                        true,
                    )
                    .await;
                let page = text(&driver);
                for title in TITLES {
                    if page.contains(title) && !seen.contains(&title) {
                        seen.push(title);
                    }
                }
            }
            assert_eq!(
                seen.len(),
                26,
                "all bound actions must be reachable: {seen:?}"
            );
            assert!(text(&driver).contains(TITLES[25]));
            assert_eq!(scroll.position().offset, scroll.position().maximum());
            driver
                .drive(
                    &mut frames,
                    mode,
                    vec![
                        (key(KeyCode::PageUp), true),
                        (key(KeyCode::Up), true),
                        (key(KeyCode::Down), true),
                    ],
                    true,
                )
                .await;
            assert!(scroll.position().offset > 0);
            let before = scroll.position().offset;
            let wheel = TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::ScrollUp,
                20,
                12,
            ));
            driver
                .drive(&mut frames, mode, vec![(wheel, true)], true)
                .await;
            assert_eq!(scroll.position().offset, before - 1);
            let closed = driver
                .drive(&mut frames, mode, vec![(key(KeyCode::Esc), true)], true)
                .await;
            assert_eq!(closed.last().unwrap().snapshot.0.len(), 1);
            assert!(!text(&driver).contains("Close Help"));
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn raw_unit_separator_opens_help_with_printable_bindings_in_help_and_menu() {
    let handle = HelpHandle::default();
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(HelpScene(journal,handle:handle.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let raw = TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('\u{1f}')));
        driver
            .drive_checked(
                &mut frames,
                InputMode::Burst,
                vec![(raw, true)],
                true,
                |ack| {
                    assert_eq!(
                        ack.snapshot.0.len(),
                        2,
                        "raw US must open Help before redraw"
                    )
                },
            )
            .await;
        assert!(text(&driver).contains("Ctrl+7"));
        assert!(
            driver
                .frames
                .last()
                .unwrap()
                .cells
                .iter()
                .flatten()
                .all(|cell| !cell.text.chars().any(char::is_control))
        );
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(key(KeyCode::Esc), true)],
                true,
            )
            .await;
        let fixture = handle.0.lock().unwrap().as_ref().unwrap().clone();
        fixture
            .stack
            .open_overlay(
                menu_overlay(
                    fixture.stack,
                    &fixture.stack.snapshot().top().actions,
                    TargetId::for_action(&action("help")),
                )
                .unwrap(),
            )
            .unwrap();
        driver.initial(&mut frames).await;
        let menu = text(&driver);
        assert!(
            menu.lines()
                .any(|line| line.contains("Help") && line.contains("F1")),
            "{menu}"
        );
        assert!(
            driver
                .frames
                .last()
                .unwrap()
                .cells
                .iter()
                .flatten()
                .all(|cell| !cell.text.chars().any(char::is_control))
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
