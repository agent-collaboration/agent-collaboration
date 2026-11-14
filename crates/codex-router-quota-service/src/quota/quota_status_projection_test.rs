use super::*;
use crate::quota_display_facts::QuotaAccountStatus;

#[test]
fn absent_credentials_require_login_even_without_a_maintenance_failure() {
    let mut row = observed_row();
    row.active_credential_generation = None;
    row.oauth_maintenance = None;
    let report = report(vec![row]);
    let view = quota_status_view_model(&report, report.rows(), 120);
    assert_eq!(
        view.rows[0].login_state,
        crate::quota_display_model::QuotaLoginState::SignedOut
    );
}

#[test]
fn login_projection_keeps_store_routing_and_generation_evidence_separate() {
    use crate::quota_display_model::QuotaLoginState;
    use codex_router_state::credential_maintenance::{
        CredentialMaintenanceRecord, CredentialMaintenanceState,
    };
    let mut row = observed_row();
    row.account_status = "disabled".to_owned();
    row.availability = AccountAvailability::Blocked;
    row.oauth_maintenance = Some(CredentialMaintenanceRecord {
        credential_generation: 0,
        state: CredentialMaintenanceState::ReauthRequired,
        failure_class: None,
        last_success_unix_seconds: None,
        next_attempt_unix_seconds: None,
        claimed_successor_generation: None,
        claim_purpose: None,
        claim_started_unix_seconds: None,
        claim_prior_state: None,
        consecutive_failures: 0,
    });
    let mut report = report(vec![row]);
    let view = quota_status_view_model(&report, report.rows(), 120);
    assert_eq!(
        view.rows[0].login_state,
        QuotaLoginState::SignedIn,
        "old maintenance is not current login evidence; routing exclusion is independent"
    );
    assert!(view.rows[0].details.login.contains("saved credentials"));
    assert!(
        view.rows[0]
            .details
            .login
            .contains("live provider validation not checked")
    );
    for availability in [
        CredentialStoreAvailability::KeychainLocked,
        CredentialStoreAvailability::Unavailable,
    ] {
        report.credential_store_availability = availability;
        let view = quota_status_view_model(&report, report.rows(), 120);
        assert_eq!(view.rows[0].login_state, QuotaLoginState::Unknown);
        assert!(view.rows[0].details.login.starts_with("Sign-in unknown"));
    }
}

#[test]
fn login_primary_fact_comes_from_typed_credential_maintenance() {
    use crate::quota_display_model::QuotaLoginState;
    use codex_router_state::credential_maintenance::{
        CredentialMaintenanceRecord, CredentialMaintenanceState,
    };
    for (state, expected) in [
        (
            CredentialMaintenanceState::Healthy,
            QuotaLoginState::SignedIn,
        ),
        (
            CredentialMaintenanceState::Retrying,
            QuotaLoginState::Retrying,
        ),
        (
            CredentialMaintenanceState::InProgress,
            QuotaLoginState::InProgress,
        ),
        (
            CredentialMaintenanceState::ReauthRequired,
            QuotaLoginState::Required,
        ),
        (
            CredentialMaintenanceState::Unrefreshable,
            QuotaLoginState::Required,
        ),
    ] {
        let mut row = observed_row();
        row.oauth_maintenance = Some(CredentialMaintenanceRecord {
            credential_generation: 1,
            state,
            failure_class: None,
            last_success_unix_seconds: None,
            next_attempt_unix_seconds: None,
            claimed_successor_generation: None,
            claim_purpose: None,
            claim_started_unix_seconds: None,
            claim_prior_state: None,
            consecutive_failures: 0,
        });
        let report = report(vec![row]);
        let view = quota_status_view_model(&report, report.rows(), 120);
        assert_eq!(view.rows[0].login_state, expected, "{state:?}");
    }
    let report = report(vec![observed_row()]);
    assert_eq!(
        quota_status_view_model(&report, report.rows(), 120).rows[0].login_state,
        QuotaLoginState::SignedIn
    );
}

#[test]
fn row_projection_carries_typed_active_clients_independently_from_display_text() {
    for count in [None, Some(0), Some(9)] {
        let mut row = observed_row();
        row.active_clients_value = count;
        row.active_clients = "unrelated presentation".to_owned();
        let report = report(vec![row]);
        let view = quota_status_view_model(&report, report.rows(), 120);
        assert_eq!(view.rows[0].active_clients_value, count);
    }
}

fn observed_row() -> QuotaStatusRow {
    QuotaStatusRow {
        provider: codex_router_core::provider::Provider::Openai,
        account_id: AccountId::new("projection-account").unwrap(),
        active_credential_generation: Some(1),
        account_label: "Projection account".to_owned(),
        account_status: "enabled".to_owned(),
        short_window: String::new(),
        weekly_window: String::new(),
        pace: String::new(),
        burn: String::new(),
        updated: String::new(),
        active_clients: "3 clients".to_owned(),
        active_clients_value: Some(3),
        active_clients_source: "fixture",
        reset_credits_available: "unknown".to_owned(),
        reset_credits_available_value: None,
        credit_usage: CreditUsageStatus::default(),
        routing: String::new(),
        next_use: String::new(),
        weekly_pace: None,
        windows: Vec::new(),
        availability: AccountAvailability::Usable,
        freshness: QuotaEvidenceFreshness::Fresh,
        routing_exclusion: RoutingExclusion::None,
        quota_evidence_reason: QuotaEvidenceReason::Ok,
        routing_reason: RoutingReason::AvailableSamePool,
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
    }
}
fn report(rows: Vec<QuotaStatusRow>) -> QuotaStatusReport {
    QuotaStatusReport {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        route_band: "responses".to_owned(),
        selected_pool: SelectedPool::Usable,
        preferred_next_account_id: None,
        selection_projection_source: SelectionProjectionSource::SqlxProjection,
        now_unix_seconds: 0,
        credential_store_availability: CredentialStoreAvailability::Ready,
        rows,
    }
}
#[test]
fn luna_422_unknown_freshness_preserves_available_routing_status() {
    let mut row = observed_row();
    row.freshness = QuotaEvidenceFreshness::Unknown;
    assert_eq!(quota_account_status(&row).label(), "available");
}

#[test]
fn luna_422_unknown_freshness_preserves_held_routing_status() {
    let mut row = observed_row();
    row.freshness = QuotaEvidenceFreshness::Unknown;
    row.availability = AccountAvailability::Reserve;
    row.routing_reason = RoutingReason::HeldReserve;
    assert_eq!(quota_account_status(&row).label(), "held");
}

#[test]
fn luna_422_unknown_freshness_preserves_blocked_routing_status() {
    for availability in [
        AccountAvailability::Blocked,
        AccountAvailability::Excluded,
        AccountAvailability::Unknown,
    ] {
        let mut row = observed_row();
        row.freshness = QuotaEvidenceFreshness::Unknown;
        row.availability = availability;
        assert_eq!(quota_account_status(&row).label(), "blocked");
    }
}

#[test]
fn luna_422_preferred_and_stale_precedence_remains() {
    let mut row = observed_row();
    row.freshness = QuotaEvidenceFreshness::Stale;
    assert_eq!(quota_account_status(&row).label(), "stale");
    row.preferred_next = true;
    assert_eq!(quota_account_status(&row).label(), "preferred");
}

#[test]
fn luna_422_unknown_freshness_json_bytes_are_unchanged() {
    let mut row = observed_row();
    row.freshness = QuotaEvidenceFreshness::Unknown;
    let mut report = report(vec![row]);
    report.app_version = "fixture-version".to_owned();
    let bytes = serde_json::to_vec(
        &super::super::quota_status_json::JsonQuotaStatusReport::from_report(&report),
    )
    .unwrap();
    assert_eq!(bytes, include_bytes!("quota_unknown_freshness.json"));
}

#[test]
fn row_projection_preserves_preference_freshness_and_credential_override_precedence() {
    for (preferred, freshness, availability, reason, expected) in [
        (
            true,
            QuotaEvidenceFreshness::Stale,
            AccountAvailability::Reserve,
            RoutingReason::HeldReserve,
            QuotaAccountStatus::Preferred,
        ),
        (
            false,
            QuotaEvidenceFreshness::Stale,
            AccountAvailability::Usable,
            RoutingReason::AvailableSamePool,
            QuotaAccountStatus::Stale,
        ),
        (
            false,
            QuotaEvidenceFreshness::Unknown,
            AccountAvailability::Usable,
            RoutingReason::AvailableSamePool,
            QuotaAccountStatus::Available,
        ),
        (
            false,
            QuotaEvidenceFreshness::Fresh,
            AccountAvailability::Usable,
            RoutingReason::AvailableSamePool,
            QuotaAccountStatus::Available,
        ),
        (
            false,
            QuotaEvidenceFreshness::Fresh,
            AccountAvailability::Reserve,
            RoutingReason::HeldReserve,
            QuotaAccountStatus::Held,
        ),
        (
            false,
            QuotaEvidenceFreshness::Fresh,
            AccountAvailability::Blocked,
            RoutingReason::UnknownFallbackAvailable,
            QuotaAccountStatus::Blocked,
        ),
    ] {
        let mut row = observed_row();
        row.preferred_next = preferred;
        row.freshness = freshness;
        row.availability = availability;
        row.routing_reason = reason;
        let mut report = report(vec![row.clone()]);
        assert_eq!(
            display_quota_row_status(&report, &row),
            QuotaRowStatus::Account(expected)
        );
        assert_eq!(
            quota_selected_account_view_model(&report, &row).status,
            expected.label()
        );
        report.credential_store_availability = CredentialStoreAvailability::KeychainLocked;
        assert_eq!(
            display_quota_row_status(&report, &row),
            QuotaRowStatus::CredentialUnavailable {
                notice: report.credential_store_availability.status_label()
            }
        );
    }
}
#[test]
fn header_projects_whole_pool_even_when_visible_rows_are_filtered_and_status_is_stale() {
    let mut stale_usable = observed_row();
    stale_usable.freshness = QuotaEvidenceFreshness::Stale;
    let mut fresh_reserve = observed_row();
    fresh_reserve.account_id = AccountId::new("reserve-account").unwrap();
    fresh_reserve.availability = AccountAvailability::Reserve;
    fresh_reserve.routing_reason = RoutingReason::HeldReserve;
    fresh_reserve.active_clients_value = Some(0);
    let report = report(vec![stale_usable.clone(), fresh_reserve]);
    let projected = quota_status_view_model(&report, &[stale_usable], 120);
    assert_eq!(projected.rows.len(), 1);
    assert_eq!(
        projected.header.pool,
        QuotaPoolCensus {
            usable: 1,
            reserve: 1,
            blocked: 0,
            unknown: 0,
            excluded: 0
        }
    );
    assert_eq!(
        projected.header.freshness,
        QuotaFreshnessCensus {
            fresh: 1,
            stale: 1,
            unknown: 0
        }
    );
    assert_eq!(projected.header.observed_clients, Some(3));
    assert_eq!(
        projected.rows[0].status,
        QuotaRowStatus::Account(QuotaAccountStatus::Stale)
    );
}
