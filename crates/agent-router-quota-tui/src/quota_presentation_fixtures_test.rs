// Typed fake quota facts shared by the presentation scenarios.
fn quota_view_model() -> QuotaStatusViewModel {
    let selected_details = selected_account_details("ssdev", "safest quota");
    QuotaStatusViewModel {
        width: 100,
        route_line: "responses -> ssdev    [preferred]".to_owned(),
        header: test_header_facts(1, 1),
        why_line: "why: safest quota".to_owned(),
        pool_freshness_summary: "fresh 1".to_owned(),
        selection_projection_degraded: false,
        serving_clients: None,
        rows: vec![QuotaStatusAccountViewModel {
            login_state: Default::default(),
            provider: codex_router_core::provider::Provider::Openai,
            account_id: test_account_id("ssdev"),
            account_tag: "test-tag".to_owned(),
            active_credential_generation: Some(1),
            enabled: true,
            selected: true,
            account: "ssdev".to_owned(),
            status: codex_router_quota_service::quota_display_facts::QuotaRowStatus::Account(
                codex_router_quota_service::quota_display_facts::QuotaAccountStatus::Available,
            ),
            active_clients: "1 client".to_owned(),
            active_clients_value: Some(1),
            reset_credits: "2 resets".to_owned(),
            credit_usage_summary: "unknown".to_owned(),
            credit_usage: codex_router_quota_service::quota::CreditUsageStatus::default(),
            reason: "safest quota".to_owned(),
            display_weekly: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
                remaining_percent: 83,
                reset_in_seconds: Some(604800),
                stale: false,
            },
            display_short: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
                remaining_percent: 99,
                reset_in_seconds: Some(18000),
                stale: false,
            },
            weekly_window: "█████ 83%\u{a0}left reset\u{a0}7d".to_owned(),
            short_window: "█████ 99%\u{a0}left reset\u{a0}5h".to_owned(),
            burn_meter: "■□□□".to_owned(),
            sample_metadata: SampleMetadata {
                confidence: SampleConfidence::Fresh,
                age_label: "14s".to_owned(),
                age_seconds: Some(14),
                semantic_label: "sample fresh",
            },
            reset_pace: ResetPaceViewModel {
                state: ResetPaceState::Healthy,
                multiple_label: "1.00x reset pace".to_owned(),
                impact_label: None,
                semantic_label: "healthy",
                meter_left_segments: ResetPaceMeterSegments {
                    filled: 0,
                    empty: 7,
                },
                meter_right_segments: ResetPaceMeterSegments {
                    filled: 0,
                    empty: 7,
                },
                center_marker: '│',
                unavailable_reason: None,
            },
            weekly_pace: "ahead reset by 2d".to_owned(),
            weekly_quota_floor_percent: 0,
            details: selected_details.clone(),
        }],
        selected: Some(selected_details),
    }
}

fn quota_empty_view_model() -> QuotaStatusViewModel {
    QuotaStatusViewModel {
        width: 100,
        route_line: "responses -> none    [empty]".to_owned(),
        header: test_header_facts(0, 0),
        why_line: "why: no accounts configured".to_owned(),
        pool_freshness_summary: "unknown".to_owned(),
        selection_projection_degraded: false,
        serving_clients: None,
        rows: Vec::new(),
        selected: None,
    }
}

fn quota_error_view_model() -> QuotaStatusViewModel {
    QuotaStatusViewModel {
        width: 100,
        route_line: "responses -> unavailable    [error]".to_owned(),
        header: codex_router_quota_service::quota_display_facts::QuotaHeaderFacts {
            credential_notice: Some("quota status unavailable".to_owned()),
            ..test_header_facts(0, 0)
        },
        why_line: "why: quota status unavailable".to_owned(),
        pool_freshness_summary: "unknown".to_owned(),
        selection_projection_degraded: true,
        serving_clients: None,
        rows: Vec::new(),
        selected: None,
    }
}

pub(crate) fn quota_two_account_view_model() -> QuotaStatusViewModel {
    let alpha_details = selected_account_details("alpha", "alpha detail");
    let beta_details = selected_account_details("beta", "beta detail");
    QuotaStatusViewModel {
        width: 120,
        route_line: "responses -> alpha    [preferred]".to_owned(),
        header: test_header_facts(2, 2),
        why_line: "why: alpha detail".to_owned(),
        pool_freshness_summary: "fresh 2".to_owned(),
        selection_projection_degraded: false,
        serving_clients: None,
        rows: vec![
            QuotaStatusAccountViewModel {
                login_state: Default::default(),
                provider: codex_router_core::provider::Provider::Openai,
                account_id: test_account_id("alpha"),
                account_tag: "alpha-tag".to_owned(),
                active_credential_generation: Some(1),
                enabled: true,
                selected: true,
                account: "alpha".to_owned(),
                status: codex_router_quota_service::quota_display_facts::QuotaRowStatus::Account(
                    codex_router_quota_service::quota_display_facts::QuotaAccountStatus::Available,
                ),
                active_clients: "1 client".to_owned(),
                active_clients_value: Some(1),
                reset_credits: "2 resets".to_owned(),
                credit_usage_summary: "unknown".to_owned(),
                credit_usage: codex_router_quota_service::quota::CreditUsageStatus::default(),
                reason: "alpha detail".to_owned(),
                display_weekly: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
                    remaining_percent: 83,
                    reset_in_seconds: Some(604800),
                    stale: false,
                },
                display_short: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
                    remaining_percent: 99,
                    reset_in_seconds: Some(18000),
                    stale: false,
                },
                weekly_window: "█████ 83%\u{a0}left reset\u{a0}7d".to_owned(),
                short_window: "█████ 99%\u{a0}left reset\u{a0}5h".to_owned(),
                burn_meter: "■□□□".to_owned(),
                sample_metadata: SampleMetadata::default(),
                reset_pace: ResetPaceViewModel::default(),
                weekly_pace: "ahead reset by 2d".to_owned(),
                weekly_quota_floor_percent: 0,
                details: alpha_details.clone(),
            },
            QuotaStatusAccountViewModel {
                login_state: Default::default(),
                provider: codex_router_core::provider::Provider::Openai,
                account_id: test_account_id("beta"),
                account_tag: "beta-tag".to_owned(),
                active_credential_generation: Some(1),
                enabled: true,
                selected: false,
                account: "beta".to_owned(),
                status: codex_router_quota_service::quota_display_facts::QuotaRowStatus::Account(
                    codex_router_quota_service::quota_display_facts::QuotaAccountStatus::Available,
                ),
                active_clients: "0 clients".to_owned(),
                active_clients_value: Some(0),
                reset_credits: "2 resets".to_owned(),
                credit_usage_summary: "unknown".to_owned(),
                credit_usage: codex_router_quota_service::quota::CreditUsageStatus::default(),
                reason: "beta detail".to_owned(),
                display_weekly: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
                    remaining_percent: 75,
                    reset_in_seconds: Some(518400),
                    stale: false,
                },
                display_short: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
                    remaining_percent: 70,
                    reset_in_seconds: Some(14400),
                    stale: false,
                },
                weekly_window: "████ 75%\u{a0}left reset\u{a0}6d".to_owned(),
                short_window: "████ 70%\u{a0}left reset\u{a0}4h".to_owned(),
                burn_meter: "■■□□".to_owned(),
                sample_metadata: SampleMetadata::default(),
                reset_pace: ResetPaceViewModel::default(),
                weekly_pace: "behind reset by 1d".to_owned(),
                weekly_quota_floor_percent: 0,
                details: beta_details,
            },
        ],
        selected: Some(alpha_details),
    }
}

fn quota_many_account_view_model() -> QuotaStatusViewModel {
    let selected_details = selected_account_details("acct00", "primary");
    let mut rows = Vec::new();
    for index in 0..10 {
        let account = format!("acct{index:02}");
        let details = selected_account_details(&account, &format!("account {index:02} detail"));
        rows.push(QuotaStatusAccountViewModel {
            login_state: Default::default(),
            provider: codex_router_core::provider::Provider::Openai,
            account_id: test_account_id(&account),
            account_tag: format!("tag-{index:02}"),
            active_credential_generation: Some(1),
            enabled: true,
            selected: index == 0,
            account,
            status: codex_router_quota_service::quota_display_facts::QuotaRowStatus::Account(
                codex_router_quota_service::quota_display_facts::QuotaAccountStatus::Available,
            ),
            active_clients: format!("{index} clients"),
            active_clients_value: Some(index),
            reset_credits: "2 resets".to_owned(),
            credit_usage_summary: "unknown".to_owned(),
            credit_usage: codex_router_quota_service::quota::CreditUsageStatus::default(),
            reason: format!("account {index:02} detail"),
            display_weekly: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
                remaining_percent: 83,
                reset_in_seconds: Some(604800),
                stale: false,
            },
            display_short: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
                remaining_percent: 99,
                reset_in_seconds: Some(18000),
                stale: false,
            },
            weekly_window: "█████ 83%\u{a0}left reset\u{a0}7d".to_owned(),
            short_window: "█████ 99%\u{a0}left reset\u{a0}5h".to_owned(),
            burn_meter: "■□□□".to_owned(),
            sample_metadata: SampleMetadata::default(),
            reset_pace: ResetPaceViewModel::default(),
            weekly_pace: "ahead reset by 2d".to_owned(),
            weekly_quota_floor_percent: 0,
            details,
        });
    }
    QuotaStatusViewModel {
        width: 160,
        route_line: "responses -> acct00    [preferred]".to_owned(),
        header: test_header_facts(10, 10),
        why_line: "why: primary".to_owned(),
        pool_freshness_summary: "fresh 10".to_owned(),
        selection_projection_degraded: false,
        serving_clients: None,
        rows,
        selected: Some(selected_details),
    }
}

fn quota_no_authoritative_selection_view_model() -> QuotaStatusViewModel {
    let mut view_model = quota_view_model();
    view_model.route_line = "responses -> none    blocked    no selectable account".to_owned();
    view_model.why_line = "why: no usable accounts".to_owned();
    view_model.selected = None;
    view_model.header.pool = codex_router_quota_service::quota_display_facts::QuotaPoolCensus {
        blocked: 1,
        ..Default::default()
    };
    for row in &mut view_model.rows {
        row.selected = false;
        row.status = codex_router_quota_service::quota_display_facts::QuotaRowStatus::Account(
            codex_router_quota_service::quota_display_facts::QuotaAccountStatus::Blocked,
        );
        row.reason = "quota ineligible".to_owned();
        row.weekly_window = "░░░░░░░░░░ 0% left, reset 7d".to_owned();
        row.reset_pace = ResetPaceViewModel::default();
        row.details.status = "blocked".to_owned();
        row.details.reason = "quota ineligible".to_owned();
        row.details.weekly_window = "░░░░░░░░░░ 0% left, reset 7d".to_owned();
        row.details.reset_pace = ResetPaceViewModel::default();
        row.details.total_rate = "rate unknown".to_owned();
        row.details.connection_rate = "not measured (unknown)".to_owned();
        row.details.guards = "5h 100% / weekly 100%".to_owned();
        row.details.note = "quota ineligible".to_owned();
    }
    view_model
}

fn selected_account_details(account: &str, reason: &str) -> QuotaSelectedAccountViewModel {
    QuotaSelectedAccountViewModel {
        account: account.to_owned(),
        status: "available".to_owned(),
        reason: reason.to_owned(),
        short_window: "█████ 99% left, reset 5h".to_owned(),
        weekly_window: "████ 83% left, reset 7d".to_owned(),
        burn_meter: "■□□□".to_owned(),
        burn_pace: "ahead reset by 2d".to_owned(),
        sample_metadata: SampleMetadata {
            confidence: SampleConfidence::Fresh,
            age_label: "14s".to_owned(),
            age_seconds: Some(14),
            semantic_label: "sample fresh",
        },
        reset_pace: ResetPaceViewModel {
            state: ResetPaceState::Healthy,
            multiple_label: "1.00x reset pace".to_owned(),
            impact_label: None,
            semantic_label: "healthy",
            meter_left_segments: ResetPaceMeterSegments {
                filled: 0,
                empty: 7,
            },
            meter_right_segments: ResetPaceMeterSegments {
                filled: 0,
                empty: 7,
            },
            center_marker: '│',
            unavailable_reason: None,
        },
        short_reset_pace: ResetPaceViewModel {
            state: ResetPaceState::OverBurning,
            multiple_label: "2.50x reset pace".to_owned(),
            impact_label: Some("runs out 2d 16h".to_owned()),
            semantic_label: "over",
            meter_left_segments: ResetPaceMeterSegments {
                filled: 0,
                empty: 7,
            },
            meter_right_segments: ResetPaceMeterSegments {
                filled: 7,
                empty: 0,
            },
            center_marker: '│',
            unavailable_reason: None,
        },
        total_rate: "0.10%/h".to_owned(),
        connection_rate: "0.05%/h/conn".to_owned(),
        active_clients: "1 client".to_owned(),
        guards: "5h 0% / weekly 8%".to_owned(),
        reset: "2 available".to_owned(),
        note: reason.to_owned(),
    }
}

fn quota_state_color_view_model() -> QuotaStatusViewModel {
    let healthy_details = selected_account_details("healthy", "healthy");
    QuotaStatusViewModel {
        width: 160,
        route_line: "responses -> healthy    [preferred]".to_owned(),
        header: test_header_facts(3, 3),
        why_line: "why: reset pace colors".to_owned(),
        pool_freshness_summary: "fresh 1".to_owned(),
        selection_projection_degraded: false,
        serving_clients: None,
        rows: vec![
            quota_state_color_row("healthy", true, ResetPaceState::Healthy, "1.00x reset pace"),
            quota_state_color_row(
                "under",
                false,
                ResetPaceState::UnderBurning,
                "0.50x reset pace",
            ),
            quota_state_color_row(
                "over",
                false,
                ResetPaceState::OverBurning,
                "1.50x reset pace",
            ),
        ],
        selected: Some(healthy_details),
    }
}

fn quota_state_color_row(
    account: &str,
    selected: bool,
    state: ResetPaceState,
    multiple_label: &str,
) -> QuotaStatusAccountViewModel {
    let semantic_label = match state {
        ResetPaceState::UnderBurning => "under",
        ResetPaceState::Healthy => "healthy",
        ResetPaceState::OverBurning => "over",
        ResetPaceState::Unavailable => "burn unavailable",
    };
    let meter_segments = if matches!(
        state,
        ResetPaceState::Healthy | ResetPaceState::UnderBurning
    ) {
        (
            ResetPaceMeterSegments {
                filled: 0,
                empty: 7,
            },
            ResetPaceMeterSegments {
                filled: 0,
                empty: 7,
            },
        )
    } else {
        (
            ResetPaceMeterSegments {
                filled: 0,
                empty: 7,
            },
            ResetPaceMeterSegments {
                filled: 4,
                empty: 3,
            },
        )
    };
    let reset_pace = ResetPaceViewModel {
        state,
        multiple_label: multiple_label.to_owned(),
        impact_label: None,
        semantic_label,
        meter_left_segments: meter_segments.0,
        meter_right_segments: meter_segments.1,
        center_marker: '│',
        unavailable_reason: None,
    };
    QuotaStatusAccountViewModel {
        login_state: Default::default(),
        provider: codex_router_core::provider::Provider::Openai,
        account_id: test_account_id(account),
        account_tag: format!("{account}-tag"),
        active_credential_generation: Some(1),
        enabled: true,
        selected,
        account: account.to_owned(),
        status: codex_router_quota_service::quota_display_facts::QuotaRowStatus::Account(
            codex_router_quota_service::quota_display_facts::QuotaAccountStatus::Available,
        ),
        active_clients: "0 clients".to_owned(),
        active_clients_value: Some(0),
        reset_credits: "2 resets".to_owned(),
        credit_usage_summary: "unknown".to_owned(),
        credit_usage: codex_router_quota_service::quota::CreditUsageStatus::default(),
        reason: semantic_label.to_owned(),
        display_weekly: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
            remaining_percent: 83,
            reset_in_seconds: Some(604800),
            stale: false,
        },
        display_short: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
            remaining_percent: 99,
            reset_in_seconds: Some(18000),
            stale: false,
        },
        weekly_window: "█████ 83%\u{a0}left reset\u{a0}7d".to_owned(),
        short_window: "█████ 99%\u{a0}left reset\u{a0}5h".to_owned(),
        burn_meter: String::new(),
        sample_metadata: SampleMetadata {
            confidence: SampleConfidence::Fresh,
            age_label: "0s".to_owned(),
            age_seconds: Some(0),
            semantic_label: "sample fresh",
        },
        reset_pace: reset_pace.clone(),
        weekly_pace: String::new(),
        weekly_quota_floor_percent: 0,
        details: QuotaSelectedAccountViewModel {
            reset_pace,
            ..selected_account_details(account, semantic_label)
        },
    }
}

fn test_account_id(value: &str) -> AccountId {
    AccountId::new(format!("test-{value}"))
        .unwrap_or_else(|error| panic!("test account id should be valid: {error}"))
}
