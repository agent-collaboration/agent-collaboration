use crate::{
    test_support::help_scene::{HelpHandle, HelpScene, TITLES, key, text},
    test_support::{
        event_driver::{EventDriver, InputMode},
        recording_backend::recording_terminal_render_loop,
    },
};
use agent_collaboration_tui::dialog::help::{HelpContent, HelpContextLine, HelpInputConvention};
use iocraft::prelude::*;
#[tokio::test]
async fn context_and_input_conventions_scroll_with_the_generated_action_list() {
    let content = HelpContent {
        context: vec![
            HelpContextLine {
                label: "Account".into(),
                value: "account-42".into(),
            },
            HelpContextLine {
                label: "Label".into(),
                value: "Full captured label for this account".into(),
            },
        ],
        conventions: vec![
            HelpInputConvention {
                key: "Arrows".into(),
                description: "Move selection".into(),
            },
            HelpInputConvention {
                key: "PgUp / PgDn".into(),
                description: "Move a page".into(),
            },
            HelpInputConvention {
                key: "Home / End".into(),
                description: "First or last choice".into(),
            },
            HelpInputConvention {
                key: "Typing".into(),
                description: "Filter the list".into(),
            },
            HelpInputConvention {
                key: "Backspace".into(),
                description: "Edit the query".into(),
            },
            HelpInputConvention {
                key: "id: / b: / repo:".into(),
                description: "Search syntax".into(),
            },
            HelpInputConvention {
                key: "Esc ladder".into(),
                description: "Clear, close, return".into(),
            },
        ],
    };
    let handle = HelpHandle::default();
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(HelpScene(journal,handle:handle.clone(),content));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let opened = driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(key(KeyCode::F(1)), true)],
                true,
            )
            .await;
        let first = text(&driver);
        assert!(first.contains("account-42"), "{first}");
        assert!(
            first.contains("Full captured label for this account"),
            "{first}"
        );
        let fixture = handle.0.lock().unwrap().clone().unwrap();
        let scroll = fixture
            .control
            .scroll(opened.last().unwrap().snapshot.0[1].0)
            .unwrap();
        assert!(scroll.position().maximum() > 0);
        let mut pages = vec![first];
        for _ in 0..scroll
            .position()
            .maximum()
            .div_ceil(scroll.position().viewport_height.max(1))
        {
            driver
                .drive(
                    &mut frames,
                    InputMode::Burst,
                    vec![(key(KeyCode::PageDown), true)],
                    true,
                )
                .await;
            pages.push(text(&driver));
        }
        let all = pages.join("\n");
        assert!(all.contains("Input conventions"), "{all}");
        assert!(all.contains("Esc ladder"), "{all}");
        assert!(all.contains("Clear, close, return"), "{all}");
        assert!(all.contains("Search syntax"), "{all}");
        for title in TITLES {
            assert!(all.contains(title), "generated action missing: {title}");
        }
        assert!(
            all.lines()
                .any(|line| line.contains("F8") && line.contains("Execute"))
        );
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap();
}
