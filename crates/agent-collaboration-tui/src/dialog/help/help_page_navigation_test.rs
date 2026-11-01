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
