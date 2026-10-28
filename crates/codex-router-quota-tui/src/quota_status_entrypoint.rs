use std::io;
use std::io::Write;

use iocraft::prelude::*;

use codex_router_quota_service::quota_reset::reset_session_supervisor::ResetSessionPorts;

use super::CreditUsagePolicySaver;
use super::CreditUsageRefresher;
use super::quota_browse_rendering::colorize_reset_pace_ansi;
use super::quota_floor_editor::WeeklyQuotaFloorSaver;
use super::quota_provider_filters::QuotaInteractionMode;
use super::quota_status_component::LIVE_QUOTA_STATUS_RELOAD_INTERVAL;
use super::quota_status_component::LIVE_QUOTA_STATUS_SPINNER_INTERVAL;
use super::quota_status_component::MIN_QUOTA_WIDTH;
use super::quota_status_component::QuotaStatusComponent;
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

pub async fn run_quota_status_view(
    view_model: QuotaStatusViewModel,
    reload_view_model: Option<QuotaStatusViewModelLoader>,
    reset_session_ports: Option<ResetSessionPorts>,
    weekly_floor_saver: Option<WeeklyQuotaFloorSaver>,
    credit_policy_saver: Option<CreditUsagePolicySaver>,
    credit_usage_refresher: Option<CreditUsageRefresher>,
) -> io::Result<()> {
    let (reset_intent_sender, reset_snapshot_receiver) = reset_session_ports
        .map(|ports| (Some(ports.intent_sender), Some(ports.snapshot_receiver)))
        .unwrap_or((None, None));
    element! {
        QuotaStatusComponent(
            view_model: view_model,
            width: 0usize,
            height: 0usize,
            reload_view_model,
            reset_intent_sender,
            reset_snapshot_receiver,
            weekly_floor_saver,
            credit_policy_saver,
            credit_usage_refresher,
            reload_interval: LIVE_QUOTA_STATUS_RELOAD_INTERVAL,
            spinner_interval: LIVE_QUOTA_STATUS_SPINNER_INTERVAL,
        )
    }
    .render_loop()
    .fullscreen()
    .ignore_ctrl_c()
    .await
}
