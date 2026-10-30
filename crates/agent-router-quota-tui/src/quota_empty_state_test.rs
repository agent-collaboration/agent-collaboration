use super::*;
use codex_router_quota_service::quota_display_model::QuotaDisplayWindow;

fn interactive_row(row: &QuotaStatusAccountViewModel) -> String {
    super::super::quota_account_groups::render_interactive_account(row, 120, false)
        .render(None)
        .to_string()
}

#[test]
fn luna_422_missing_windows_and_extras_are_empty() {
    let mut model = quota_view_model();
    let row = &mut model.rows[0];
    row.display_short = QuotaDisplayWindow::Unknown;
    row.reset_credits = "resets unknown".to_owned();
    let frame = interactive_row(row);
    for placeholder in ["No data", "5h · —", "Credits unknown", "resets unknown"] {
        assert!(!frame.contains(placeholder), "{placeholder}: {frame}");
    }
    assert!(frame.contains("83% · week · 7d 0h"), "{frame}");
}

#[test]
fn luna_422_unknown_reset_omits_separator_and_dash() {
    let mut model = quota_view_model();
    let row = &mut model.rows[0];
    row.display_weekly = QuotaDisplayWindow::Known {
        remaining_percent: 32,
        reset_in_seconds: None,
        stale: true,
    };
    row.display_short = QuotaDisplayWindow::Known {
        remaining_percent: 100,
        reset_in_seconds: None,
        stale: false,
    };
    let frame = interactive_row(row);
    for value in ["32% · week", "100% · 5h", "week stale"] {
        assert!(frame.contains(value), "{frame}");
    }
    assert!(!frame.contains("· —"), "{frame}");
}

#[test]
fn luna_422_unavailable_pace_is_two_cell_hourglass_without_meter() {
    assert_eq!(unicode_width::UnicodeWidthStr::width("⏳"), 2);
    let mut model = quota_view_model();
    model.rows[0].reset_pace = ResetPaceViewModel::default();
    let frame = interactive_row(&model.rows[0]);
    assert!(frame.contains("⏳"), "{frame}");
    assert!(!frame.contains("burn unavailable"), "{frame}");
    assert!(!frame.contains('□') && !frame.contains('■'), "{frame}");
}

#[test]
fn luna_422_weekly_meter_is_third_line_in_pace_column() {
    let model = quota_view_model();
    let frame = interactive_row(&model.rows[0]);
    let lines = frame.lines().collect::<Vec<_>>();
    let (first, label) = lines
        .iter()
        .enumerate()
        .find(|(_, line)| line.contains("healthy"))
        .unwrap();
    let origin = unicode_width::UnicodeWidthStr::width(label.split_once("healthy").unwrap().0);
    for (offset, value) in [(1, "1.00x reset pace"), (2, "□□□□□□□│□□□□□□□")]
    {
        let line = lines.get(first + offset).expect("pace line");
        assert!(line.contains(value), "{frame}");
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(line.split_once(value).unwrap().0),
            origin,
            "{frame}"
        );
    }
}

#[test]
fn luna_422_unknown_freshness_routes_through_both_real_text_renderers() {
    use codex_router_quota_service::quota::{
        CredentialStoreAvailability, CreditUsageStatus, QuotaStatusReport, QuotaStatusRow,
        SelectionProjectionSource,
    };
    use codex_router_selection::burn_down::{
        AccountAvailability, QuotaEvidenceFreshness, QuotaEvidenceReason, RoutingExclusion,
        RoutingReason, SelectedPool,
    };
    use codex_router_selection::run_rate::QuotaRunRateConfidence;
    for (availability, reason, label, glyph) in [
        (
            AccountAvailability::Usable,
            RoutingReason::AvailableSamePool,
            "available",
            "✓",
        ),
        (
            AccountAvailability::Reserve,
            RoutingReason::HeldReserve,
            "held",
            "◇",
        ),
        (
            AccountAvailability::Blocked,
            RoutingReason::BlockedWindowExhausted,
            "blocked",
            "⊘",
        ),
    ] {
        let row = QuotaStatusRow {
            provider: codex_router_core::provider::Provider::Openai,
            account_id: test_account_id("routing-fixture"),
            active_credential_generation: Some(1),
            account_label: "routing-fixture".to_owned(),
            account_status: "enabled".to_owned(),
            short_window: String::new(),
            weekly_window: String::new(),
            pace: String::new(),
            burn: String::new(),
            updated: String::new(),
            active_clients: "0 clients".to_owned(),
            active_clients_value: Some(0),
            active_clients_source: "fixture",
            reset_credits_available: "unknown".to_owned(),
            reset_credits_available_value: None,
            credit_usage: CreditUsageStatus::default(),
            routing: String::new(),
            next_use: String::new(),
            weekly_pace: None,
            windows: Vec::new(),
            availability,
            freshness: QuotaEvidenceFreshness::Unknown,
            routing_exclusion: RoutingExclusion::None,
            quota_evidence_reason: QuotaEvidenceReason::Ok,
            routing_reason: reason,
            preferred_next: false,
            short_pressure: 0,
            long_pressure: 0,
            short_salvage: 0,
            long_salvage: 0,
            limiting_window: None,
            weekly_survival_margin_basis_points: None,
            weekly_projected_exhaustion_unix_seconds: None,
            weekly_burn_rate_confidence: QuotaRunRateConfidence::Unknown,
            weekly_quota_floor_basis_points: None,
            oauth_maintenance: None,
        };
        let report = QuotaStatusReport {
            app_version: "fixture".to_owned(),
            route_band: "responses".to_owned(),
            selected_pool: SelectedPool::Usable,
            preferred_next_account_id: None,
            selection_projection_source: SelectionProjectionSource::SqlxProjection,
            now_unix_seconds: 0,
            credential_store_availability: CredentialStoreAvailability::Ready,
            rows: vec![row],
        };
        let model =
            codex_router_quota_service::quota::quota_status_projection::quota_status_view_model(
                &report,
                report.rows(),
                120,
            );
        let interactive = interactive_row(&model.rows[0]);
        assert!(
            interactive.contains(&format!("{glyph} {label}")),
            "{interactive}"
        );
        assert!(!interactive.contains("? unknown"), "{interactive}");
        let mut plain = Vec::new();
        crate::write_quota_status_view(&mut plain, model, false).unwrap();
        let plain = String::from_utf8(plain).unwrap();
        let primary = plain
            .lines()
            .find(|line| line.contains("routing-fixture"))
            .unwrap();
        assert!(primary.contains(label), "{plain}");
        assert!(
            plain.contains("no data"),
            "missing quota remains in the plain window fields: {plain}"
        );
    }
}
