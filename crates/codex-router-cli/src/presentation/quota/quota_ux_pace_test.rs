//! Compact pace wording must fit without changing full Details or wider rows.
use super::quota_ux_frame_test::phone_account_region;
use super::*;

#[tokio::test]
async fn phone_pace_uses_whole_compact_words_and_preserves_full_details() {
    let mut observations = Vec::new();
    for semantic_label in ["burn unavailable", "collecting data"] {
        let mut model = super::super::quota_storyboard_fixture_test::fixture_model();
        model.rows[0].reset_pace.semantic_label = semantic_label;
        model.rows[0].details.reset_pace = model.rows[0].reset_pace.clone();
        let details = render_quota_static_capture(model.clone(), 160, false);
        assert!(
            quota_selected_panel_text(&details).contains(semantic_label),
            "Details must retain the full distinct pace meaning: {details}"
        );
        let frame =
            render_quota_capture_model_at(model, 32, 80, vec![provider_filter_key(KeyCode::Esc)])
                .await;
        let account = phone_account_region(&frame, "東京", Some("Claude"));
        let quota = account
            .replace(['│', '┃'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        println!("PHONE PACE {semantic_label} ACTUAL32:\n{frame}");
        assert!(
            frame
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= 32)
        );
        observations.push((semantic_label, quota));
    }
    for (semantic_label, quota) in &observations {
        assert!(
            quota.contains(semantic_label),
            "C1 folded pace retains full semantic meaning and Details; source={semantic_label}, observations={observations:?}, quota={quota}"
        );
    }
}

#[tokio::test]
async fn phone_pace_wider_rows_keep_full_words_and_observable_frames() {
    for width in [40usize, 48, 76, 120] {
        let mut model = super::super::quota_storyboard_fixture_test::fixture_model();
        model.rows[1].reset_pace.semantic_label = "collecting data";
        let frame = render_quota_capture_model_at(
            model,
            width,
            80,
            vec![provider_filter_key(KeyCode::Esc)],
        )
        .await;
        assert!(
            frame.contains("unavailable") && frame.contains("collecting"),
            "{frame}"
        );
        assert!(
            frame
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width)
        );
        println!("PHONE PACE UNCHANGED {width} BEGIN\n{frame}\nPHONE PACE UNCHANGED {width} END");
    }
}
