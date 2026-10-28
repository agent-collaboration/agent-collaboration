use super::*;
#[cfg(test)]
use crate::quota::quota_status_rendering::write_quota_table;

#[test]
fn quota_status_width_contract_preserves_layout() {
    let report = quota_capture_report();

    for width in [48, 72, 90, 120] {
        let mut output = Vec::new();
        must_ok(write_quota_table(&mut output, &report, Some(width)));
        let text = must_ok(String::from_utf8(output));
        assert_quota_capture_width_contract(width, &text);
    }

    let blocked_report = blocked_quota_capture_report();
    let mut output = Vec::new();
    must_ok(write_quota_table(&mut output, &blocked_report, Some(80)));
    let text = must_ok(String::from_utf8(output));
    assert!(
        text.contains("responses · 2 accounts · blocked 2"),
        "blocked capture should expose the complete pool summary:\n{text}"
    );
    assert!(
        text.lines().all(|line| line.chars().count() <= 80),
        "blocked quota capture overflowed:\n{text}"
    );
}

#[test]
fn quota_status_empty_windows_keep_weekly_bar_and_show_exhausted_reset_pace() {
    let mut report = blocked_quota_capture_report();
    for row in &mut report.rows {
        for window in &mut row.windows {
            window.status = QuotaWindowStatus::Ineligible;
        }
    }
    let mut output = Vec::new();

    must_ok(write_quota_table(&mut output, &report, Some(120)));
    let text = must_ok(String::from_utf8(output));

    assert!(
        text.contains("░░░░░░░░░░ 0% left, reset 7d"),
        "depleted weekly quota should keep its quota bar and reset hint:\n{text}"
    );
    assert!(text.contains("Exhausted"), "{text}");
    assert!(
        !text.contains("🅇  Exhausted"),
        "depleted reset pace should not include the icon marker:\n{text}"
    );
    assert!(
        !text.contains("runs out now"),
        "depleted reset pace should not show old runout copy:\n{text}"
    );
}

#[test]
fn quota_status_terminal_color_keeps_exhausted_red() {
    let report = blocked_quota_capture_report();
    let mut output = Vec::new();

    must_ok(write_quota_table_with_style(
        &mut output,
        &report,
        Some(120),
        QuotaTableStyle::TerminalColor,
    ));
    let text = must_ok(String::from_utf8(output));

    assert!(
        text.contains("\u{1b}[38;5;9mExhausted"),
        "exhausted quota label should keep the red over-burning color:\n{text:?}"
    );
}

#[test]
fn quota_status_table_separates_quota_bars_from_burn_bars() {
    let report = quota_capture_report();
    let mut output = Vec::new();

    must_ok(write_quota_table(&mut output, &report, Some(120)));
    let text = must_ok(String::from_utf8(output));

    assert!(
        !text.contains("  Account") && !text.contains("Status") && !text.contains("Pace"),
        "account list should not render table headers:\n{text}"
    );
    assert!(
        text.contains("Quota windows") && text.contains("Reset pace"),
        "selected account details should separate quota windows from reset pace:\n{text}"
    );
    assert!(
        text.contains("%/h") && text.contains("%/h/conn"),
        "quota table should expose total and per-connection rate units:\n{text}"
    );
    for account_label in ["ssdev", "askluna", "matches", "legacy"] {
        assert_quota_capture_account_windows(&text, account_label);
    }
    assert!(
        text.contains("Weekly pace"),
        "account list should end with weekly burndown:\n{text}"
    );
    assert!(
        text.contains("Reset pace"),
        "selected details should retain reset pace diagnostics:\n{text}"
    );
    assert!(
        !text.contains("current [")
            && !text.contains("safe pace")
            && !text.contains("ahead to reset")
            && !text.contains("safe pace unknown"),
        "quota table should not use legacy burn/safe-pace copy:\n{text}"
    );
}

#[test]
fn quota_status_table_shows_stale_values_with_sample_marker_without_refresh_filler() {
    let mut report = quota_capture_report();
    let row = report
        .rows
        .get_mut(0)
        .unwrap_or_else(|| panic!("capture report should include a selected row"));
    for window in &mut row.windows {
        window.status = QuotaWindowStatus::Stale;
        window.observed_unix_seconds = NOW - 901;
    }
    row.short_window = format_window_cell(&row.windows, V1_SHORT_WINDOW_SECONDS, NOW, true);
    row.weekly_window = format_window_cell(&row.windows, V1_WEEKLY_WINDOW_SECONDS, NOW, true);
    row.freshness = QuotaEvidenceFreshness::Stale;

    let mut output = Vec::new();
    must_ok(write_quota_table(&mut output, &report, Some(120)));
    let text = must_ok(String::from_utf8(output));

    assert!(text.contains("█") && text.contains("% left"), "{text}");
    assert!(text.contains("sample stale 15m 1s"), "{text}");
    assert!(
        !text.contains("needs refresh"),
        "stale value-bearing status output should show values and mark sample stale once:\n{text}"
    );
}

#[test]
fn weekly_quota_floor_has_stable_json_plain_and_tui_observer_fields() {
    let mut report = quota_capture_report();
    let row = report
        .rows
        .get_mut(0)
        .unwrap_or_else(|| panic!("capture report should include a selected row"));
    row.weekly_quota_floor_basis_points = Some(1_500);
    row.routing_exclusion = RoutingExclusion::WeeklyQuotaFloor;
    row.quota_evidence_reason = QuotaEvidenceReason::WeeklyQuotaFloor;
    row.routing_reason = RoutingReason::ExcludedWeeklyQuotaFloor;
    row.routing = format_routing_reason(row.routing_reason).to_owned();
    row.next_use = format_next_use_from_routing_reason(row.routing_reason).to_owned();

    let mut json_output = Vec::new();
    must_ok(write_quota_json(&mut json_output, &report));
    let json: serde_json::Value = must_ok(serde_json::from_slice(&json_output));
    assert_eq!(
        json["accounts"][0]["weekly_quota_floor_basis_points"],
        1_500
    );
    assert_eq!(json["accounts"][0]["weekly_quota_floor_percent"], 15);
    assert_eq!(
        json["accounts"][0]["weekly_quota_switch_at_basis_points"],
        1_800
    );
    assert_eq!(json["accounts"][0]["weekly_quota_switch_at_percent"], 18);
    assert_eq!(
        json["accounts"][0]["routing_exclusion"],
        "excluded_weekly_quota_floor"
    );
    assert_eq!(
        json["accounts"][0]["routing_reason"],
        "excluded_weekly_quota_floor"
    );

    let mut plain_output = Vec::new();
    must_ok(write_quota_plain(&mut plain_output, &report));
    let plain = must_ok(String::from_utf8(plain_output));
    assert!(plain.contains("weekly floor"));
    assert!(plain.contains("\tswitches at 18% / floor 15%\t"));
    assert!(
        json["accounts"][0]
            .get("weekly_quota_effective_stop_basis_points")
            .is_none()
    );
    assert!(plain.contains("blocked: weekly quota floor"));

    let view_model = quota_status_view_model(&report, report.rows(), 120);
    let selected = view_model
        .selected
        .unwrap_or_else(|| panic!("capture should include selected details"));
    assert_eq!(selected.reason, "weekly quota floor");
    assert!(selected.guards.contains("floor 15%"));
    assert!(selected.guards.contains("switches at 18%"));

    for width in [48, 160] {
        let mut tui_output = Vec::new();
        must_ok(write_quota_table(&mut tui_output, &report, Some(width)));
        let text = must_ok(String::from_utf8(tui_output));
        assert!(
            text.contains("floor 15%"),
            "weekly floor should remain visible at width {width}:\n{text}"
        );
    }
}

#[test]
fn quota_status_table_can_emit_terminal_color() {
    let report = quota_capture_report();
    let mut output = Vec::new();

    must_ok(write_quota_table_with_style(
        &mut output,
        &report,
        Some(120),
        QuotaTableStyle::TerminalColor,
    ));
    let text = must_ok(String::from_utf8(output));

    assert!(
        text.contains("\x1b["),
        "quota table should emit ANSI styling:\n{text:?}"
    );
    assert!(
        text.contains("\x1b[38;5;11m") && text.contains("pace under"),
        "quota pace should emit state color:\n{text:?}"
    );
    assert!(
        !text.contains("\x1b[32m"),
        "quota status should avoid the old mixed green/yellow status palette:\n{text:?}"
    );
    assert!(
        !text.contains("\x1b[48;2;58;70;122m"),
        "quota colors should not use the old blue selected-row background:\n{text:?}"
    );
}

#[test]
#[ignore = "writes visual quota capture artifacts for design review"]
fn quota_status_capture_artifacts_for_design_review() {
    let capture_dir = capture_dir();

    for case in QuotaCaptureDesignCase::ALL {
        let report = quota_capture_case_report(case);
        for width in [48, 160] {
            let mut output = Vec::new();
            must_ok(write_quota_table(&mut output, &report, Some(width)));
            let text = must_ok(String::from_utf8(output));
            let mut ansi_output = Vec::new();
            must_ok(write_quota_table_with_style(
                &mut ansi_output,
                &report,
                Some(width),
                QuotaTableStyle::TerminalColor,
            ));
            let ansi_text = must_ok(String::from_utf8(ansi_output));
            write_capture_pair_with_svg_text(
                &capture_dir,
                &format!("{}-{width}", case.file_stem()),
                &text,
                &ansi_text,
            );
        }
    }
}

#[test]
fn quota_list_unit_groups_keep_typed_notes_and_actual_narrow_lines() {
    let mut report = quota_capture_report();
    report.rows.truncate(1);
    let row = &mut report.rows[0];
    row.reset_credits_available_value = Some(2);
    row.weekly_pace = None;
    row.windows = vec![
        display_window(
            V1_WEEKLY_WINDOW_SECONDS,
            83,
            NOW + 7 * 86400,
            QuotaRunRateEstimate::unknown(),
        ),
        display_window(
            V1_SHORT_WINDOW_SECONDS,
            99,
            NOW + 5 * 3600,
            QuotaRunRateEstimate::unknown(),
        ),
    ];
    let weekly = &row.windows[0];
    assert_eq!(window_display_note(weekly, NOW), "resets in 7d");
    for width in [48usize, 76, 120] {
        let mut model = quota_status_view_model(&report, report.rows(), width);
        model.selected = None;
        let mut output = Vec::new();
        must_ok(codex_router_quota_tui::write_quota_status_view(
            &mut output,
            model,
            false,
        ));
        let frame = must_ok(String::from_utf8(output)).replace('\u{a0}', " ");
        println!("TYPED UNIT FRAME{width}:\n{frame}");
        for fact in ["2 resets", "83% left", "resets 7d", "99% left", "resets 5h"] {
            assert!(
                frame.lines().any(|line| line.contains(fact)),
                "complete fitting fact {fact} missing at{width}: {frame}"
            );
        }
        assert!(frame.contains("burn unavailable"), "{frame}");
        assert!(
            !frame.contains("□□□□□□□│□□□□□□□"),
            "no-data graphic adds no measurement: {frame}"
        );
        assert!(
            frame.contains('█'),
            "actual quota meters must remain: {frame}"
        );
    }
}

#[test]
fn quota_list_unit_projection_preserves_measured_pace_graphics() {
    let report = quota_capture_report();
    for state in [
        ResetPaceState::UnderBurning,
        ResetPaceState::Healthy,
        ResetPaceState::OverBurning,
    ] {
        let mut model = quota_status_view_model(&report, report.rows(), 120);
        model.rows.truncate(1);
        let pace = &mut model.rows[0].reset_pace;
        pace.state = state;
        pace.meter_left_segments.filled = 2;
        pace.meter_right_segments.filled = 3;
        pace.impact_label = None;
        pace.multiple_label = "1.00x".to_owned();
        pace.semantic_label = "measured pace";
        let mut output = Vec::new();
        must_ok(codex_router_quota_tui::write_quota_status_view(
            &mut output,
            model,
            false,
        ));
        let frame = must_ok(String::from_utf8(output));
        assert!(
            frame.contains("□□□□□■■│■■■□□□□")
                && frame.contains("1.00x")
                && frame.contains("measured pace"),
            "actual measured pace must remain for{state:?}: {frame}"
        );
    }
}
