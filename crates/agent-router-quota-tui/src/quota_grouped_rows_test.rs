//! Typed grouping preserves source order within each bucket and filters before counting.
use super::*;
use crate::{
    QuotaGroupFacet,
    quota_grouped_rows::grouped_account_rows,
    quota_provider_filters::{
        AccountProviderFilter, DisabledAccountVisibility, QuotaAccountFilters,
    },
};
use agent_collaboration_tui::controls::list::ListRows;
use codex_router_core::provider::Provider;
use codex_router_quota_service::quota_display_facts::{QuotaAccountStatus, QuotaRowStatus};

#[test]
fn provider_groups_count_filtered_rows_and_sum_typed_clients_without_parsing_labels() {
    let base = quota_view_model().rows.remove(0);
    let mut rows = Vec::new();
    for (index, provider, enabled, clients) in [
        (0, Provider::Claude, true, Some(3)),
        (1, Provider::Openai, true, Some(2)),
        (2, Provider::Claude, false, Some(100)),
        (3, Provider::Openai, true, Some(4)),
    ] {
        let mut row = base.clone();
        row.account_id = test_account_id(&format!("group-{index}"));
        row.provider = provider;
        row.enabled = enabled;
        row.active_clients_value = clients;
        row.active_clients = "display label is deliberately unrelated".to_owned();
        rows.push(row);
    }
    let filters = QuotaAccountFilters {
        group_by: QuotaGroupFacet::Provider,
        disabled: DisabledAccountVisibility::Hidden,
        ..Default::default()
    };
    let ListRows::Grouped(groups) = grouped_account_rows(filters, &rows).unwrap() else {
        panic!("grouped projection")
    };
    assert_eq!(
        groups
            .iter()
            .map(|group| group.heading.name.as_str())
            .collect::<Vec<_>>(),
        ["OpenAI", "Claude"]
    );
    assert_eq!(
        groups
            .iter()
            .map(|group| group.heading.count)
            .collect::<Vec<_>>(),
        [2, 1]
    );
    assert_eq!(
        groups[0].heading.summary.as_deref(),
        Some("6 active clients")
    );
    assert_eq!(
        groups[1].heading.summary.as_deref(),
        Some("3 active clients")
    );
    assert_eq!(
        groups[0]
            .keys
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        [rows[1].account_id.as_str(), rows[3].account_id.as_str()]
    );
    let ListRows::Grouped(filtered) = grouped_account_rows(
        QuotaAccountFilters {
            provider: AccountProviderFilter::Provider(Provider::Claude),
            ..filters
        },
        &rows,
    )
    .unwrap() else {
        panic!("grouped projection")
    };
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].heading.name, "Claude");
    assert_eq!(filtered[0].heading.count, 1);
    rows[1].active_clients_value = None;
    let ListRows::Grouped(unknown) = grouped_account_rows(filters, &rows).unwrap() else {
        panic!("grouped projection")
    };
    assert_eq!(
        unknown[0].heading.summary.as_deref(),
        Some("Active clients unknown")
    );
    let ListRows::Ungrouped(plain) =
        grouped_account_rows(QuotaAccountFilters::default(), &rows).unwrap()
    else {
        panic!("ungrouped projection")
    };
    assert_eq!(
        plain.iter().map(|key| key.as_str()).collect::<Vec<_>>(),
        rows.iter()
            .map(|row| row.account_id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        grouped_account_rows(filters, &[]).unwrap(),
        ListRows::Grouped(Vec::new())
    );
}

#[test]
fn status_groups_follow_best_to_worst_and_keep_equal_status_source_order() {
    let base = quota_view_model().rows.remove(0);
    let statuses = [
        QuotaRowStatus::Account(QuotaAccountStatus::Stale),
        QuotaRowStatus::Account(QuotaAccountStatus::Available),
        QuotaRowStatus::CredentialUnavailable {
            notice: "credentials locked".to_owned(),
        },
        QuotaRowStatus::Account(QuotaAccountStatus::Blocked),
        QuotaRowStatus::Account(QuotaAccountStatus::Preferred),
        QuotaRowStatus::Account(QuotaAccountStatus::Held),
        QuotaRowStatus::Account(QuotaAccountStatus::Available),
    ];
    let rows = statuses
        .into_iter()
        .enumerate()
        .map(|(index, status)| {
            let mut row = base.clone();
            row.account_id = test_account_id(&format!("status-{index}"));
            row.status = status;
            row
        })
        .collect::<Vec<_>>();
    let ListRows::Grouped(groups) = grouped_account_rows(
        QuotaAccountFilters {
            group_by: QuotaGroupFacet::Status,
            ..Default::default()
        },
        &rows,
    )
    .unwrap() else {
        panic!("grouped projection")
    };
    assert_eq!(
        groups
            .iter()
            .map(|group| group.heading.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Preferred",
            "Available",
            "Held",
            "Blocked",
            "Stale",
            "Credentials unavailable"
        ]
    );
    assert_eq!(
        groups[1]
            .keys
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>(),
        [rows[1].account_id.as_str(), rows[6].account_id.as_str()]
    );
    assert_eq!(
        groups
            .iter()
            .map(|group| group.heading.count)
            .sum::<usize>(),
        rows.len()
    );
}
