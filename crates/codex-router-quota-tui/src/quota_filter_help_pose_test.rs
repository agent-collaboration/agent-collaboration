//! Mounted poses for the compact filter Help controls and unchanged draft return.
use super::*;

fn has_complete_field_control(frame: &str) -> bool {
    let help = super::quota_ux_clarity_test::actual_dialog_text(frame, "Quota Help");
    let letters = help
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .collect::<String>();
    help.contains('↑') && help.contains('↓') && letters.contains("Movefieldorchoicefocus")
}

#[tokio::test]
async fn compact_filter_help_controls_are_reachable_and_preserve_draft() {
    for (width, height, expected_down_count) in [
        (120usize, 36usize, 0),
        (76, 30, 0),
        (48, 24, 3),
        (48, 12, 8),
        (40, 24, 6),
        (32, 24, 5),
    ] {
        let mut model = quota_two_account_view_model();
        // Literal header facts in the compact fixture's retained Help frame.
        model.header.pool = codex_router_quota_service::quota_display_facts::QuotaPoolCensus {
            usable: 0,
            reserve: 0,
            blocked: 0,
            unknown: 1,
            excluded: 3,
        };
        model.header.freshness =
            codex_router_quota_service::quota_display_facts::QuotaFreshnessCensus {
                fresh: 0,
                stale: 0,
                unknown: 4,
            };
        model.header.observed_clients = Some(0);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            let initial =
                super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap()
                    .to_string();
            let mut before = String::new();
            for key in [
                KeyCode::F(3),
                KeyCode::Enter,
                KeyCode::Down,
                KeyCode::Enter,
                KeyCode::Down,
                KeyCode::Up,
            ] {
                sender.send(provider_filter_key(key)).unwrap();
                before = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap()
                    .to_string();
            }
            assert!(before.contains("Account filters") && before.contains("OpenAI"));
            sender.send(provider_filter_key(KeyCode::F(1))).unwrap();
            let mut help =
                super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap()
                    .to_string();
            let mut down_count = 0;
            while !has_complete_field_control(&help) && down_count < 128 {
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                help = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap()
                    .to_string();
                down_count += 1;
            }
            assert!(
                has_complete_field_control(&help),
                "{width}x{height}: {help}"
            );
            assert_eq!(down_count, expected_down_count, "mounted Help control pose");
            assert!(help.contains("Close"));
            println!("FILTER_HELP_POSE {width}x{height} Down={down_count}\n{help}");
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            let returned =
                super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap()
                    .to_string();
            assert_eq!(
                returned, before,
                "Help preserves the exact filter draft/focus"
            );
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            let cancelled =
                super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap()
                    .to_string();
            assert_eq!(
                cancelled, initial,
                "cancel preserves committed filter state"
            );
        };
        tokio::time::timeout(Duration::from_secs(3), journey)
            .await
            .unwrap();
    }
}
