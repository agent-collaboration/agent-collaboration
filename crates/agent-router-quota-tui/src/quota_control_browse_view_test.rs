//! Real browse renders must leave spare height after the last account.
use crate::quota_status_component::QuotaStatusComponent;
use codex_router_core::{ids::AccountId, provider::Provider};
use iocraft::prelude::*;

fn assert_stacked_accounts(frame: &str, width: usize, selected_index: usize) {
    // Scope the oracle to the list: the wide sidecar repeats the selected name.
    let rows = frame
        .lines()
        .map(|line| line.split('│').nth(2).unwrap_or(""))
        .collect::<Vec<_>>();
    let tops = ["alpha-gap", "beta-gap", "gamma-gap"].map(|label| {
        rows.iter()
            .position(|line| line.contains(label))
            .unwrap_or_else(|| panic!("missing list account {label}:\n{frame}"))
    });
    // Name, provider/activity, login, routing, then two empty lines. At 100
    // cells the provider/client caption wraps, adding one line to each account.
    let (expected, account_height) = if width == 180 {
        ([6, 12, 19], 4)
    } else {
        ([6, 13, 21], 5)
    };
    assert_eq!(tops, expected, "{width}-cell list:\n{frame}");
    assert!(rows[5].contains("OpenAI (2)"), "{frame}");
    assert!(rows[tops[2] - 1].contains("Claude (1)"), "{frame}");
    for top in tops {
        assert!(
            rows[top + account_height - 2].contains("Signed in"),
            "{frame}"
        );
        assert!(
            rows[top + account_height - 1].contains("available"),
            "{frame}"
        );
        assert!(rows[top + account_height].trim().is_empty(), "{frame}");
        assert!(rows[top + account_height + 1].trim().is_empty(), "{frame}");
    }
    assert!(rows[tops[selected_index]].contains('❯'), "{frame}");
}

async fn render_grouped_account_spacing(width: usize, height: usize) {
    // Arrange: two accounts in the first group and one in the second.
    let mut model = crate::quota_browse_presentation_test::quota_two_account_view_model();
    let template = model.rows[0].clone();
    model.rows = [
        (
            "019a0000-0000-7000-8000-000000000001",
            "alpha-gap",
            Provider::Openai,
        ),
        (
            "019a0000-0000-7000-8000-000000000002",
            "beta-gap",
            Provider::Openai,
        ),
        (
            "019a0000-0000-7000-8000-000000000003",
            "gamma-gap",
            Provider::Claude,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (identity, label, provider))| {
        let mut row = template.clone();
        row.account_id = AccountId::new(identity).unwrap();
        row.account = label.to_owned();
        row.details.account = label.to_owned();
        row.provider = provider;
        row.selected = index == 0;
        row
    })
    .collect();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    // Act: mount the production screen, grouping, list and iocraft layout.
    let mut component = element!(QuotaStatusComponent(
        view_model:model, width, height,
        initial_group_facet:crate::QuotaGroupFacet::Provider
    ));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let canvas = crate::quota_frame_observation::next_current_frame(frames.as_mut())
        .await
        .unwrap()
        .unwrap();
    let initial = canvas.to_string();
    println!("GROUPED GAP INITIAL {width}x{height}:\n{initial}");
    assert_stacked_accounts(&initial, width, 0);

    // Act: selection must repaint without introducing stale row geometry.
    sender
        .send(TerminalEvent::Key(KeyEvent::new(
            KeyEventKind::Press,
            KeyCode::Down,
        )))
        .unwrap();
    let canvas = crate::quota_frame_observation::next_current_frame(frames.as_mut())
        .await
        .unwrap()
        .unwrap();
    let selected = canvas.to_string();
    println!("GROUPED GAP SELECTED {width}x{height}:\n{selected}");
    // Assert: the marker acknowledges the action and spacing remains exact.
    assert_stacked_accounts(&selected, width, 1);
}

#[tokio::test]
async fn grouped_account_rows_stack_from_top_at_wide_size() {
    render_grouped_account_spacing(180, 48).await;
}

#[tokio::test]
async fn grouped_account_rows_stack_from_top_at_small_size() {
    render_grouped_account_spacing(100, 32).await;
}
