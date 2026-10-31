//! Map stable account identities into the shared list's selection and navigation.
use super::quota_control_context::{QuotaControlContext, QuotaControlResult, action_id};
use agent_collaboration_tui::{
    controls::{
        ListNavigationPolicy, ListRowKey, ListSelectionBinding, ListSelectionControl,
        ListSelectionState,
    },
    focus_model::TargetId,
    surface_handle::{SurfaceContext, SurfaceStackHandle},
    surface_stack::SurfaceId,
};
use codex_router_core::ids::AccountId;
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;
use iocraft::prelude::*;
use std::sync::Arc;

pub(crate) fn account_list_target() -> QuotaControlResult<TargetId> {
    Ok(TargetId::for_action(&action_id("accounts")?))
}

pub(crate) fn account_row_key(account_id: &AccountId) -> QuotaControlResult<ListRowKey> {
    ListRowKey::new(account_id.as_str()).map_err(|error| error.to_string())
}

pub(crate) fn initial_account_selection(
    model: &QuotaStatusViewModel,
    selected: Option<&AccountId>,
) -> QuotaControlResult<ListSelectionState> {
    let rows = model
        .rows
        .iter()
        .map(|row| account_row_key(&row.account_id))
        .collect::<Result<Vec<_>, _>>()?;
    let selected = selected
        .map(account_row_key)
        .transpose()?
        .filter(|key| rows.contains(key));
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
    let rows = context
        .filters
        .read()
        .committed
        .visible_rows(&context.view_model.read().rows)
        .iter()
        .map(|row| account_row_key(&row.account_id))
        .collect::<Result<Vec<_>, _>>()?;
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
