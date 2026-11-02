//! Filtering precedes grouping; the shared list owns the resulting selectable order.
use super::quota_provider_filters::QuotaAccountFilters;
use agent_collaboration_tui::controls::list::{ListGroup, ListGroupHeading, ListRowKey, ListRows};
use codex_router_core::{ids::AccountId, provider::Provider};
use codex_router_quota_service::{
    quota_display_facts::{QuotaAccountStatus, QuotaRowStatus},
    quota_display_model::QuotaStatusAccountViewModel,
};
use std::collections::BTreeMap;

pub(crate) fn grouped_account_rows(
    filters: QuotaAccountFilters,
    rows: &[QuotaStatusAccountViewModel],
) -> Result<ListRows, String> {
    let visible = filters.visible_rows(rows);
    let group_key: fn(&QuotaStatusAccountViewModel) -> (u8, &'static str) = match filters.group_by {
        crate::QuotaGroupFacet::Provider => provider_group_key,
        crate::QuotaGroupFacet::Status => status_group_key,
        crate::QuotaGroupFacet::None => {
            return Ok(ListRows::Ungrouped(
                visible
                    .iter()
                    .map(|row| account_row_key(&row.account_id))
                    .collect::<Result<_, _>>()?,
            ));
        }
    };
    let mut groups = BTreeMap::<(u8, &'static str), Vec<&QuotaStatusAccountViewModel>>::new();
    for row in &visible {
        let group = group_key(row);
        groups.entry(group).or_default().push(row);
    }
    let groups = groups
        .into_iter()
        .map(|((_, name), rows)| {
            let total = rows.iter().try_fold(0_u64, |total, row| {
                row.active_clients_value
                    .map(|count| total.saturating_add(u64::from(count)))
            });
            let summary = total.map_or_else(
                || "Active clients unknown".to_owned(),
                |total| {
                    format!(
                        "{total} active {}",
                        if total == 1 { "client" } else { "clients" }
                    )
                },
            );
            Ok(ListGroup {
                heading: ListGroupHeading {
                    name: name.to_owned(),
                    count: rows.len(),
                    summary: Some(summary),
                },
                keys: rows
                    .iter()
                    .map(|row| account_row_key(&row.account_id))
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ListRows::Grouped(groups))
}

fn provider_group_key(row: &QuotaStatusAccountViewModel) -> (u8, &'static str) {
    match row.provider {
        Provider::Openai => (0, "OpenAI"),
        Provider::Claude => (1, "Claude"),
    }
}
fn status_group_key(row: &QuotaStatusAccountViewModel) -> (u8, &'static str) {
    match &row.status {
        QuotaRowStatus::Account(QuotaAccountStatus::Preferred) => (0, "Preferred"),
        QuotaRowStatus::Account(QuotaAccountStatus::Available) => (1, "Available"),
        QuotaRowStatus::Account(QuotaAccountStatus::Held) => (2, "Held"),
        QuotaRowStatus::Account(QuotaAccountStatus::Blocked) => (3, "Blocked"),
        QuotaRowStatus::Account(QuotaAccountStatus::Stale) => (4, "Stale"),
        QuotaRowStatus::CredentialUnavailable { .. } => (5, "Credentials unavailable"),
    }
}
pub(crate) fn account_row_key(account_id: &AccountId) -> Result<ListRowKey, String> {
    ListRowKey::new(account_id.as_str()).map_err(|error| error.to_string())
}
