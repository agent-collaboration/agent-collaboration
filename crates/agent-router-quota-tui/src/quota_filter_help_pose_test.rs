//! Mounted poses for the compact filter Help controls and unchanged draft return.
use super::*;
use crate::quota_test_support::frame_output::{SharedFrameOutput, dispatch_render_loop};
use crate::quota_test_support::{QuotaInputFrameDriver, current_quota_canvas};
use agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink;

fn has_complete_field_control(frame: &str) -> bool {
    let help = super::quota_ux_clarity_test::actual_dialog_text(frame, "Help");
    let letters = help
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .collect::<String>();
    help.contains("Tab/Shift+Tab") && letters.contains("Movethroughenabledcontrols")
}

#[tokio::test]
async fn compact_filter_help_controls_are_reachable_and_preserve_draft() {
    for (width, height) in [
        (120usize, 36usize),
        (76, 30),
        (48, 24),
        (48, 12),
        (40, 24),
        (32, 24),
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
        let directory = tempfile::tempdir().unwrap();
        let receipt_path = directory.path().join("filter-help-input.jsonl");
        let sink = InputDispatchReceiptSink::create(&receipt_path).unwrap();
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height,dispatch_receipt:Some(sink.clone())));
        let bytes = SharedFrameOutput::default();
        let writer = Box::new(sink.frame_writer(bytes.clone()).unwrap());
        let frames = dispatch_render_loop(
            &mut component,
            MockTerminalConfig::with_events(events),
            bytes,
            writer,
        );
        tokio::pin!(frames);
        let mut input = QuotaInputFrameDriver::new(sender, receipt_path);
        let initial = current_quota_canvas(frames.as_mut()).await.to_string();
        let mut before = String::new();
        for key in [
            KeyCode::F(3),
            KeyCode::Enter,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::BackTab,
        ] {
            before = input
                .step(frames.as_mut(), provider_filter_key(key), |_| true)
                .await
                .to_string();
        }
        assert!(before.contains("Account filters") && before.contains("OpenAI"));
        let mut help = input
            .step(frames.as_mut(), provider_filter_key(KeyCode::F(1)), |_| {
                true
            })
            .await
            .to_string();
        let mut down_count = 0;
        while !has_complete_field_control(&help) && down_count < 128 {
            help = input
                .step(frames.as_mut(), provider_filter_key(KeyCode::Down), |_| {
                    true
                })
                .await
                .to_string();
            down_count += 1;
        }
        assert!(
            has_complete_field_control(&help),
            "{width}x{height}: {help}"
        );
        assert!(down_count < 128, "bounded generated Help reachability");
        assert!(help.contains("Close"));
        println!("FILTER_HELP_POSE {width}x{height} Down={down_count}\n{help}");
        let returned = input
            .step(frames.as_mut(), provider_filter_key(KeyCode::Esc), |_| true)
            .await
            .to_string();
        assert_eq!(
            returned, before,
            "Help preserves the exact filter draft/focus"
        );
        let cancelled = input
            .step(frames.as_mut(), provider_filter_key(KeyCode::Esc), |_| true)
            .await
            .to_string();
        assert_eq!(
            cancelled, initial,
            "cancel preserves committed filter state"
        );
        sink.flush_and_check().unwrap();
    }
}
