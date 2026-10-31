use std::io;
use std::io::Write;

use iocraft::prelude::*;

use codex_router_quota_service::quota_reset::reset_session_supervisor::ResetSessionPorts;

use super::CreditUsagePolicySaver;
use super::CreditUsageRefresher;
use super::quota_fact_presentation::colorize_reset_pace_ansi;
use super::quota_floor_editor::WeeklyQuotaFloorSaver;
use super::quota_status_component::QuotaStatusComponent;
use super::quota_view_configuration::LIVE_QUOTA_STATUS_RELOAD_INTERVAL;
use super::quota_view_configuration::LIVE_QUOTA_STATUS_SPINNER_INTERVAL;
use super::quota_view_configuration::MIN_QUOTA_WIDTH;
use super::quota_view_configuration::QuotaInteractionMode;
use crate::quota_status_view_model::QuotaStatusViewModelLoader;
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;

pub fn write_quota_status_view(
    writer: &mut impl Write,
    view_model: QuotaStatusViewModel,
    ansi: bool,
) -> io::Result<()> {
    let width = view_model.width.max(MIN_QUOTA_WIDTH);
    let height = 0usize;
    let mut element = element! {
        QuotaStatusComponent(view_model: view_model, width: width, height: height, interaction_mode: QuotaInteractionMode::Static)
    };
    let canvas = element.render(None);
    if ansi {
        let mut output = Vec::new();
        canvas.write(&mut output)?;
        let text = String::from_utf8(output)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        writer.write_all(colorize_reset_pace_ansi(&text).as_bytes())
    } else {
        canvas.write(writer)
    }
}

#[cfg(test)]
pub(crate) fn quota_static_render_height(view_model: &QuotaStatusViewModel, width: usize) -> usize {
    element!(QuotaStatusComponent(view_model:view_model.clone(),width,height:0usize,interaction_mode:QuotaInteractionMode::Static)).render(None).to_string().lines().count()
}

#[derive(Default)]
pub struct QuotaStatusViewOptions {
    pub reload_view_model: Option<QuotaStatusViewModelLoader>,
    pub reset_session_ports: Option<ResetSessionPorts>,
    pub weekly_floor_saver: Option<WeeklyQuotaFloorSaver>,
    pub credit_policy_saver: Option<CreditUsagePolicySaver>,
    pub credit_usage_refresher: Option<CreditUsageRefresher>,
    pub quota_refresher: Option<crate::QuotaRefresher>,
    pub account_commands: Option<crate::QuotaAccountCommandQueue>,
    pub focused_account_id: Option<codex_router_core::ids::AccountId>,
    pub notice: Option<String>,
}

pub async fn run_quota_status_view(
    view_model: QuotaStatusViewModel,
    options: QuotaStatusViewOptions,
) -> io::Result<crate::QuotaOutcome> {
    let (reset_intent_sender, reset_snapshot_receiver) = options
        .reset_session_ports
        .map(|ports| (Some(ports.intent_sender), Some(ports.snapshot_receiver)))
        .unwrap_or((None, None));
    let (width, height) = crossterm::terminal::size()?;
    let outcome = std::sync::Arc::new(std::sync::Mutex::new(None));
    let outcome_sink = Some(outcome.clone());
    element! {
        QuotaStatusComponent(
            view_model: view_model,
            width: usize::from(width),
            height: usize::from(height),
            reload_view_model: options.reload_view_model,
            reset_intent_sender,
            reset_snapshot_receiver,
            weekly_floor_saver:options.weekly_floor_saver,
            credit_policy_saver:options.credit_policy_saver,
            credit_usage_refresher:options.credit_usage_refresher,
            quota_refresher:options.quota_refresher,
            account_commands:options.account_commands,
            initial_focused_account_id:options.focused_account_id,
            notice:options.notice,
            outcome_sink,
            reload_interval: LIVE_QUOTA_STATUS_RELOAD_INTERVAL,
            spinner_interval: LIVE_QUOTA_STATUS_SPINNER_INTERVAL,
        )
    }
    .render_loop()
    .fullscreen()
    .ignore_ctrl_c()
    .await?;
    outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or_else(|| io::Error::other("quota exited without a terminal outcome"))
}
