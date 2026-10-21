use super::*;

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
fn quota_status_view_model_route_line_summarizes_the_account_pool() {
    let report = quota_capture_report();
    let view_model = quota_status_view_model(&report, report.rows(), 120);

    assert_eq!(
        view_model.route_line, "responses · 4 accounts · usable 2 · reserve 1 · blocked 1",
        "route summary should describe the full account pool instead of the preferred account"
    );
    assert_eq!(view_model.pool_freshness_summary, "fresh 3 · stale 1");
    assert!(view_model.why_line.is_empty());
}

#[test]
fn quota_status_view_model_reports_serving_clients_from_active_mirror() {
    let report = quota_capture_report();
    let view_model = quota_status_view_model(&report, report.rows(), 120);

    assert_eq!(view_model.serving_clients, Some(5));
}

#[test]
fn quota_provider_filters_projection_preserves_typed_provider_independent_of_labels() {
    use codex_router_core::provider::Provider;

    let mut report = quota_capture_report();
    report.rows[0].provider = Provider::Claude;
    report.rows[0].account_label = "OpenAI named account".to_owned();
    report.rows[1].provider = Provider::Openai;
    report.rows[1].account_label = "Claude named account".to_owned();

    let model = quota_status_view_model(&report, report.rows(), 120);

    assert_eq!(model.rows[0].provider, Provider::Claude);
    assert_eq!(model.rows[1].provider, Provider::Openai);
    assert_eq!(model.rows[0].account_id, report.rows[0].account_id);
    assert_eq!(model.rows[1].account_id, report.rows[1].account_id);
}

#[test]
fn quota_status_headers_summarize_every_report_row_independent_of_preference_or_visibility() {
    let mut report = quota_capture_report();
    let template = report
        .rows
        .first()
        .cloned()
        .unwrap_or_else(|| panic!("capture report should have an account row"));
    let scenarios = [
        (
            "usable",
            AccountAvailability::Usable,
            QuotaEvidenceFreshness::Fresh,
        ),
        (
            "reserve",
            AccountAvailability::Reserve,
            QuotaEvidenceFreshness::Fresh,
        ),
        (
            "blocked",
            AccountAvailability::Blocked,
            QuotaEvidenceFreshness::Stale,
        ),
        (
            "unknown",
            AccountAvailability::Unknown,
            QuotaEvidenceFreshness::Unknown,
        ),
        (
            "excluded",
            AccountAvailability::Excluded,
            QuotaEvidenceFreshness::Stale,
        ),
    ];
    report.rows = scenarios
        .into_iter()
        .enumerate()
        .map(|(index, (label, availability, freshness))| {
            let mut row = template.clone();
            row.account_id = AccountId::new(format!("account_{label}"))
                .expect("fixture account identity should validate");
            row.account_label = label.to_owned();
            row.availability = availability;
            row.freshness = freshness;
            row.preferred_next = index == 0;
            row
        })
        .collect();
    report.preferred_next_account_id = report.rows.first().map(|row| row.account_id.clone());
    report.selection_projection_source = SelectionProjectionSource::DisplayWindowsFallback;

    let full = quota_status_view_model(&report, report.rows(), 120);
    assert_eq!(
        full.route_line,
        "responses · degraded · 5 accounts · usable 1 · reserve 1 · blocked 1 · unknown 1 · excluded 1"
    );
    assert_eq!(full.pool_freshness_summary, "fresh 2 · stale 2 · unknown 1");
    assert!(full.selection_projection_degraded);

    let clipped_rows = report.rows[..1].to_vec();
    report.rows[4].preferred_next = true;
    report.preferred_next_account_id = report.rows.get(4).map(|row| row.account_id.clone());
    let clipped = quota_status_view_model(&report, &clipped_rows, 48);
    assert_eq!(clipped.route_line, full.route_line);
    assert_eq!(clipped.pool_freshness_summary, full.pool_freshness_summary);

    report.rows.clear();
    report.preferred_next_account_id = None;
    let empty = quota_status_view_model(&report, report.rows(), 48);
    assert_eq!(empty.route_line, "responses · degraded · no accounts");
    assert_eq!(empty.pool_freshness_summary, "unknown");
}

#[test]
fn degraded_projection_clears_initial_admission_preference_labels() {
    let mut report = quota_capture_report();
    let row = report
        .rows
        .get_mut(0)
        .unwrap_or_else(|| panic!("capture report should include a selected row"));
    row.preferred_next = true;
    row.routing_reason = RoutingReason::PreferredNearResetInitialAdmission;
    row.routing = format_routing_reason(row.routing_reason).to_owned();
    row.next_use = format_next_use_from_routing_reason(row.routing_reason).to_owned();

    assert_eq!(
        row.routing,
        "preferred by quota: near-reset initial admission"
    );
    assert_eq!(row.next_use, "preferred by quota");

    row.normalize_degraded_projection_authority();

    assert!(!row.preferred_next);
    assert_eq!(row.routing_reason, RoutingReason::UnknownFallbackAvailable);
    assert_eq!(row.routing, "fallback by quota: same unknown pool");
    assert_eq!(row.next_use, "fallback by quota");
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
        must_ok(crate::presentation::quota::write_quota_status_view(
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
fn quota_list_unit_notes_preserve_unknown_stale_and_past_reset_meaning() {
    for (status, reset, expected_note, expected_list_note) in [
        (
            QuotaWindowStatus::Stale,
            Some(NOW + 7 * 86400),
            "resets in 7d",
            "resets 7d",
        ),
        (
            QuotaWindowStatus::Eligible,
            Some(NOW - 3600),
            "resets 1h ago",
            "resets 1h ago",
        ),
        (
            QuotaWindowStatus::Eligible,
            None,
            "reset unknown",
            "reset unknown",
        ),
        (
            QuotaWindowStatus::Unknown,
            None,
            "unknown; needs refresh",
            "unknown; needs refresh",
        ),
        (
            QuotaWindowStatus::Ineligible,
            None,
            "quota ineligible",
            "quota ineligible",
        ),
    ] {
        let window = DisplayQuotaWindow {
            status,
            reset_unix_seconds: reset,
            ..display_window(
                V1_WEEKLY_WINDOW_SECONDS,
                83,
                NOW + 7 * 86400,
                QuotaRunRateEstimate::unknown(),
            )
        };
        assert_eq!(window_display_note(&window, NOW), expected_note);
        let summary =
            quota_account_list_window_summary(&[window], V1_WEEKLY_WINDOW_SECONDS, "", NOW);
        assert!(
            summary.replace('\u{a0}', " ").contains(expected_list_note),
            "{summary:?}"
        );
    }
}

#[test]
fn quota_list_unit_formatter_groups_values_at_the_typed_origin() {
    let windows = [display_window(
        V1_WEEKLY_WINDOW_SECONDS,
        83,
        NOW + 7 * 86400,
        QuotaRunRateEstimate::unknown(),
    )];
    let summary = quota_account_list_window_summary(&windows, V1_WEEKLY_WINDOW_SECONDS, "", NOW);
    assert!(
        summary.contains("83%\u{a0}left"),
        "remaining value/unit must stay together: {summary:?}"
    );
    assert!(
        summary.contains("resets\u{a0}7d"),
        "reset duration must stay attached: {summary:?}"
    );
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
        must_ok(crate::presentation::quota::write_quota_status_view(
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
