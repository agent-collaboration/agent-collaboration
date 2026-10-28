use super::*;

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
