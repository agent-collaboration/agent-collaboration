//! Static quota renderer selection.
use super::{QuotaCommandError, Write};
use agent_router_quota_tui::write_quota_status_view;
use codex_router_quota_service::quota::quota_status_projection::quota_status_view_model;
use codex_router_quota_service::quota::quota_status_view_model::QuotaStatusReport;
#[cfg(test)]
pub(super) fn write_quota_table(
    stdout: &mut impl Write,
    report: &QuotaStatusReport,
    terminal_width: Option<usize>,
) -> Result<(), QuotaCommandError> {
    write_quota_table_with_style(stdout, report, terminal_width, QuotaTableStyle::PlainText)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QuotaTableStyle {
    #[cfg(test)]
    PlainText,
    TerminalColor,
}

impl QuotaTableStyle {
    pub(super) const fn ansi(self) -> bool {
        match self {
            #[cfg(test)]
            Self::PlainText => false,
            Self::TerminalColor => true,
        }
    }
}

pub(super) fn write_quota_table_with_style(
    stdout: &mut impl Write,
    report: &QuotaStatusReport,
    terminal_width: Option<usize>,
    style: QuotaTableStyle,
) -> Result<(), QuotaCommandError> {
    let rows = report.rows();
    let width = terminal_width.unwrap_or(100).max(40);
    let view_model = quota_status_view_model(report, rows, width);
    write_quota_status_view(stdout, view_model, style.ansi()).map_err(QuotaCommandError::Stdout)
}
