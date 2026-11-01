//! Map stable account identities into the shared list's selection and navigation.
use super::quota_control_context::{QuotaControlContext, QuotaControlResult, action_id};
use super::quota_grouped_rows::account_row_key;
use agent_collaboration_tui::{
    controls::list::{
        ListNavigationPolicy, ListRowKey, ListSelectionBinding, ListSelectionControl,
        ListSelectionState,
    },
    surfaces::{
        focus_model::TargetId,
        surface_handle::{SurfaceContext, SurfaceStackHandle},
        surface_stack::SurfaceId,
    },
};
use codex_router_core::ids::AccountId;
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;
use iocraft::prelude::*;
use std::sync::Arc;

pub(crate) fn account_list_target() -> QuotaControlResult<TargetId> {
    Ok(TargetId::for_action(&action_id("accounts")?))
}

pub(crate) fn initial_account_selection(
    model: &QuotaStatusViewModel,
    selected: Option<&AccountId>,
    facet: crate::QuotaGroupFacet,
) -> QuotaControlResult<ListSelectionState> {
    let rows = super::quota_grouped_rows::grouped_account_rows(
        super::quota_provider_filters::QuotaAccountFilters {
            group_by: facet,
            ..Default::default()
        },
        &model.rows,
    )?;
    let selected = selected.map(account_row_key).transpose()?.filter(|key| {
        model
            .rows
            .iter()
            .any(|row| row.account_id.as_str() == key.as_str())
    });
    ListSelectionState::new(rows, selected).map_err(|error| error.to_string())
}

pub(crate) fn account_list_control(
    stack: SurfaceStackHandle,
    context: &QuotaControlContext,
) -> QuotaControlResult<ListSelectionControl> {
    let selected_context = context.clone();
    let height = context.height;
    Ok(ListSelectionControl::new(ListSelectionBinding {
        parent: SurfaceContext {
            stack,
            surface: SurfaceId::Screen,
        },
        id: account_list_target()?,
        value: context.account_selection,
        activation: action_id("details")?,
        page_rows: Arc::new(move || height.get().saturating_sub(7).max(1)),
        on_select: Handler::from(move |selected: Option<ListRowKey>| {
            let account = selected.and_then(|key| {
                selected_context
                    .view_model
                    .read()
                    .rows
                    .iter()
                    .find(|row| row.account_id.as_str() == key.as_str())
                    .map(|row| row.account_id.clone())
            });
            let mut focused = selected_context.focused_account_id;
            if *focused.read() != account {
                focused.set(account);
            }
        }),
    })
    .with_navigation_policy(ListNavigationPolicy::ContentScroll))
}

pub(crate) fn synchronize_account_selection(
    control: &ListSelectionControl,
    context: &QuotaControlContext,
) -> QuotaControlResult<()> {
    let rows = super::quota_grouped_rows::grouped_account_rows(
        context.filters.read().committed,
        &context.view_model.read().rows,
    )?;
    control
        .replace_rows(rows)
        .map_err(|error| error.to_string())?;
    if let Some(selected) = context.focused_account_id.read().as_ref() {
        let key = account_row_key(selected)?;
        let mut value = context.account_selection;
        let mut next = value.read().clone();
        if next.select(&key) {
            value.set(next);
        }
    }
    Ok(())
}
