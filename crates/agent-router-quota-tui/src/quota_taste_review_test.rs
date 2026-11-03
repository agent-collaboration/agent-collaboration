//! Observable claims from Lead taste reviews 01a18f3c and 01a18f46.
use super::*;
use crate::{
    quota_frame_observation::next_matching_frame,
    test_support::{QuotaFixtureControl, QuotaFixtureOptions, quota_control_fixture},
};

async fn taste_frame(control: QuotaFixtureControl, marker: &str) -> Canvas {
    let mut model = quota_two_account_view_model();
    model.header.observed_clients = Some(1);
    let mut fixture = quota_control_fixture(QuotaFixtureOptions {
        view_model: model,
        ports: crate::QuotaStatusViewOptions {
            weekly_floor_saver: Some(Arc::new(|_, _| panic!("capture cannot save"))),
            ..Default::default()
        },
        width: 120,
        height: 36,
        control,
    });
    let frames = fixture
        .element
        .mock_terminal_render_loop(MockTerminalConfig::with_events(
            futures_util::stream::pending::<TerminalEvent>(),
        ));
    tokio::pin!(frames);
    next_matching_frame(frames.as_mut(), Duration::from_secs(2), |canvas| {
        canvas.to_string().contains(marker)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn taste_browse_toolbar_retains_options_floor_counts_and_fills_the_list_height() {
    let canvas = taste_frame(QuotaFixtureControl::Browse, "Menu F10").await;
    let frame = canvas.to_string();
    let toolbar = frame
        .lines()
        .find(|line| line.contains("Menu F10"))
        .unwrap();
    let positions = ["Details", "Filters:", "Pool", "Options", "Floor"].map(|label| {
        toolbar
            .find(label)
            .unwrap_or_else(|| panic!("missing {label}: {frame}"))
    });
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "{frame}"
    );
    assert!(
        toolbar.contains("Quota · 2/2") && toolbar.contains("1 client"),
        "{frame}"
    );
    assert!(
        !frame.contains("1 clients") && !frame.contains("Quota status"),
        "{frame}"
    );
    let bottom = frame.lines().rev().nth(1).unwrap();
    assert!(
        bottom.contains('└') && bottom.contains('┘'),
        "list fills the available height: {frame}"
    );
}

#[tokio::test]
async fn taste_filters_show_a_stable_menu_title_and_declared_dialog_keys() {
    let menu = quota_menu_canvas(quota_two_account_view_model(), 120, 36)
        .await
        .to_string();
    let item = menu
        .lines()
        .find(|line| line.contains("Filters…"))
        .unwrap_or_else(|| panic!("{menu}"));
    assert!(!item.contains('▾') && !item.contains("Provider:"), "{item}");
    let filters = taste_frame(QuotaFixtureControl::Filters, "Account filters")
        .await
        .to_string();
    assert!(
        filters.contains("● Apply Ctrl+Enter") && filters.contains("Cancel Esc"),
        "{filters}"
    );
}

#[tokio::test]
async fn taste_floor_declares_save_and_cancel_keys_without_arrow_words() {
    let frame = taste_frame(QuotaFixtureControl::Floor, "Weekly floor")
        .await
        .to_string();
    assert!(
        frame.contains("● Save Ctrl+Enter") && frame.contains("Cancel Esc"),
        "{frame}"
    );
    assert!(
        frame.contains("◀ 0% ▶") && !frame.contains("◀ Left") && !frame.contains("▶ Right"),
        "{frame}"
    );
    assert!(
        frame.contains("0% disables protection · maximum 15%"),
        "{frame}"
    );
}

#[tokio::test]
async fn taste_details_discloses_the_captured_login_command() {
    let frame = taste_frame(QuotaFixtureControl::Details, "Account details")
        .await
        .to_string();
    assert!(
        frame.contains("codex-router account login --provider openai --label alpha"),
        "R17 exact command preview: {frame}"
    );
}

#[tokio::test]
async fn taste_pool_labels_the_route_and_aligns_every_census_field() {
    let canvas = render_quota_capture_model_at(
        quota_two_account_view_model(),
        120,
        36,
        vec![
            provider_filter_key(KeyCode::F(5)),
            provider_filter_key(KeyCode::Esc),
        ],
    )
    .await;
    let frame = super::quota_ux_clarity_test::actual_dialog_text(&canvas, "Pool details");
    let labels = [
        "Pool",
        "Accounts",
        "Usable",
        "Reserve",
        "Blocked",
        "Pool unknown",
        "Excluded",
        "Fresh",
        "Stale",
        "Data unknown",
        "Clients",
    ];
    let mut value_columns = Vec::new();
    for label in labels {
        let line = frame
            .lines()
            .filter(|line| !line.trim_start().starts_with("Pool details"))
            .find(|line| {
                line.split('│')
                    .any(|segment| segment.trim_start().starts_with(&format!("{label} ")))
            })
            .unwrap_or_else(|| panic!("missing {label}: {frame}"));
        let start = line.find(label).unwrap() + label.len();
        let value_offset = line
            .get(start..)
            .expect("literal label ends at a character boundary")
            .find(|character: char| !character.is_whitespace())
            .unwrap();
        value_columns.push(start + value_offset);
        assert!(!line.contains(&format!("{label}:")), "{line}");
    }
    assert!(
        value_columns.windows(2).all(|pair| pair[0] == pair[1]),
        "{frame}"
    );
    assert!(
        frame.contains("responses"),
        "route fact remains visible: {frame}"
    );
}

#[tokio::test]
async fn taste_options_close_hint_uses_escape_and_preserves_the_control_r_toggle() {
    for control in [QuotaFixtureControl::Options, QuotaFixtureControl::Credits] {
        let mut toggle = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('r'));
        toggle.modifiers = KeyModifiers::CONTROL;
        for close in [
            provider_filter_key(KeyCode::Esc),
            TerminalEvent::Key(toggle),
        ] {
            // Arrange: mount the real dialog with a saver that rejects any effect.
            let mut fixture = quota_control_fixture(QuotaFixtureOptions {
                view_model: quota_two_account_view_model(),
                ports: crate::QuotaStatusViewOptions {
                    credit_policy_saver: Some(Arc::new(|_, _, _| {
                        panic!("closing Options must not save a credit preference")
                    })),
                    ..Default::default()
                },
                width: 120,
                height: 36,
                control,
            });
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let events = futures_util::stream::unfold(receiver, |mut receiver| async {
                receiver.recv().await.map(|event| (event, receiver))
            });
            let frames = fixture
                .element
                .mock_terminal_render_loop(MockTerminalConfig::with_events(events));
            tokio::pin!(frames);
            let dialog = next_matching_frame(frames.as_mut(), Duration::from_secs(2), |canvas| {
                let frame = canvas.to_string();
                frame.contains("Options ·") && frame.contains("Close")
            })
            .await
            .unwrap()
            .to_string();
            assert!(
                dialog.contains("Close Esc") && !dialog.contains("Close Ctrl+R"),
                "{control:?} exposes the universal close hint: {dialog}"
            );

            // Act: both the shown Esc and retained Ctrl+R close this dialog.
            sender.send(close).unwrap();
            let browse = next_matching_frame(frames.as_mut(), Duration::from_secs(2), |canvas| {
                let frame = canvas.to_string();
                frame.contains("Menu F10") && !frame.contains("Options ·")
            })
            .await
            .unwrap()
            .to_string();

            // Assert: close returns to Browse without becoming Quit or saving.
            assert!(
                browse.contains("alpha") && browse.contains("beta"),
                "{browse}"
            );
            assert_eq!(*fixture.outcome.lock().unwrap(), None);
            sender
                .send(provider_filter_key(KeyCode::Char('q')))
                .unwrap();
            frames.collect::<Vec<_>>().await;
            assert_eq!(
                *fixture.outcome.lock().unwrap(),
                Some(crate::QuotaOutcome::Quit)
            );
        }
    }
}
