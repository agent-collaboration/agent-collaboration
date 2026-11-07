//! Deterministic CLI presentation for shared-host operator responses.

use std::io::Write;
use std::time::Instant;

use crate::host_command::replacement_outcome::HostRestartResult;
use codex_router_host::HostProgress;
use codex_router_host::HostSnapshot;
use codex_router_host::OperatorFrame;
use codex_router_host::UpdateResult;

pub(crate) struct HostProgressPresenter {
    active: Option<(HostProgress, Instant, indicatif::ProgressBar)>,
    tty: bool,
}

impl HostProgressPresenter {
    pub(crate) fn new(tty: bool) -> Self {
        Self { active: None, tty }
    }

    pub(crate) fn accept<W: Write>(
        &mut self,
        stdout: &mut W,
        frame: &OperatorFrame,
    ) -> std::io::Result<()> {
        match frame {
            OperatorFrame::Progress(progress) => {
                if is_phase_completion(*progress) {
                    self.finish(
                        stdout,
                        if *progress == HostProgress::RemoteControlDegraded {
                            Some(
                                codex_router_host::TerminalClassification::LocalReadyRemoteDegraded,
                            )
                        } else {
                            None
                        },
                    )?;
                    if self.active.is_none() && !self.tty {
                        writeln!(stdout, "✓ {}", progress_label(*progress))?;
                    }
                    return Ok(());
                }
                if *progress == HostProgress::AppServerKilled {
                    self.finish(
                        stdout,
                        Some(codex_router_host::TerminalClassification::Failed),
                    )?;
                    if !self.tty {
                        writeln!(stdout, "⚠ {}", progress_label(*progress))?;
                    }
                    return Ok(());
                }
                self.finish(stdout, None)?;
                let spinner = indicatif::ProgressBar::new_spinner();
                spinner.enable_steady_tick(std::time::Duration::from_millis(80));
                spinner.set_message(progress_label(*progress).to_owned());
                self.active = Some((*progress, Instant::now(), spinner));
            }
            OperatorFrame::Terminal(response) => {
                self.finish(stdout, Some(response.classification()))?
            }
        }
        Ok(())
    }

    pub(crate) fn finish_success<W: Write>(&mut self, stdout: &mut W) -> std::io::Result<()> {
        self.finish(stdout, None)
    }

    fn finish<W: Write>(
        &mut self,
        stdout: &mut W,
        terminal: Option<codex_router_host::TerminalClassification>,
    ) -> std::io::Result<()> {
        let Some((progress, started, spinner)) = self.active.take() else {
            return Ok(());
        };
        let glyph = if progress == HostProgress::AppServerKilled {
            "⚠"
        } else if matches!(
            terminal,
            Some(
                codex_router_host::TerminalClassification::Failed
                    | codex_router_host::TerminalClassification::Busy
            )
        ) {
            "✗"
        } else {
            "✓"
        };
        spinner.finish_and_clear();
        if !self.tty {
            return writeln!(
                stdout,
                "{glyph} {} ({:.2?})",
                progress_label(progress),
                started.elapsed()
            );
        }
        writeln!(
            stdout,
            "{glyph} {} ({:.2?})",
            progress_label(progress),
            started.elapsed()
        )
    }
}

pub(crate) fn render_restart_result<W: Write>(
    stdout: &mut W,
    result: &HostRestartResult,
) -> std::io::Result<()> {
    match result {
        HostRestartResult::Restarted { snapshot } => {
            writeln!(
                stdout,
                "restart_result: host restarted using installed executable"
            )?;
            render_snapshot(stdout, snapshot)
        }
        HostRestartResult::NotRestarted { response } => {
            writeln!(stdout, "restart_result: host not restarted")?;
            writeln!(
                stdout,
                "result: {}",
                classification_label(response.classification(), response.snapshot().fleet())
            )?;
            writeln!(stdout, "message: {}", response.message())?;
            render_snapshot(stdout, response.snapshot())
        }
        HostRestartResult::ReplacementFailed { message } => {
            writeln!(stdout, "restart_result: replacement host failed")?;
            writeln!(stdout, "message: {message}")?;
            writeln!(
                stdout,
                "recovery_action: after the old Host exits, run codex-router host with its original root and port"
            )
        }
    }
}

pub(crate) fn render_terminal_frame<W: Write>(
    stdout: &mut W,
    frames: &[OperatorFrame],
) -> std::io::Result<()> {
    if let Some(OperatorFrame::Terminal(response)) = frames.last() {
        writeln!(
            stdout,
            "result: {}",
            classification_label(response.classification(), response.snapshot().fleet())
        )?;
        writeln!(stdout, "message: {}", response.message())?;
        render_snapshot(stdout, response.snapshot())?;
    }
    Ok(())
}

pub(crate) fn render_progress_event<W: Write>(
    stdout: &mut W,
    progress: codex_router_host::HostProgress,
) -> std::io::Result<()> {
    let mut presenter = HostProgressPresenter::new(false);
    presenter.accept(stdout, &OperatorFrame::Progress(progress))?;
    presenter.finish_success(stdout)
}

fn progress_label(progress: HostProgress) -> &'static str {
    match progress {
        codex_router_host::HostProgress::PreparingAppServer => "preparing app-server",
        codex_router_host::HostProgress::ReplacementStarting => "starting Host replacement",
        codex_router_host::HostProgress::StoppingAppServer => "stopping app-server",
        codex_router_host::HostProgress::StartingAppServer => "starting app-server",
        codex_router_host::HostProgress::WaitingForRemoteControl => "waiting for Remote Control",
        codex_router_host::HostProgress::AppServerKilled => "forced app-server shutdown",
        codex_router_host::HostProgress::StoppingRouter => "stopping router",
        codex_router_host::HostProgress::PreparingRouter => "preparing router",
        codex_router_host::HostProgress::StartingRouter => "starting router",
        codex_router_host::HostProgress::ReExecuting => "re-executing Host",
        codex_router_host::HostProgress::RouterReady => "router ready",
        codex_router_host::HostProgress::AppServerReady => "app-server ready",
        codex_router_host::HostProgress::RemoteControlReady => "Remote Control ready",
        codex_router_host::HostProgress::RemoteControlDegraded => "Remote Control degraded",
        codex_router_host::HostProgress::UpdatingAppServer => "updating app-server",
    }
}

fn is_phase_completion(progress: HostProgress) -> bool {
    matches!(
        progress,
        HostProgress::RouterReady
            | HostProgress::AppServerReady
            | HostProgress::RemoteControlReady
            | HostProgress::RemoteControlDegraded
    )
}

pub(crate) fn render_update_result<W: Write>(
    stdout: &mut W,
    result: &UpdateResult,
) -> std::io::Result<()> {
    match result {
        UpdateResult::NoChange => writeln!(stdout, "update_result: no change"),
        UpdateResult::FailedWithoutRestart { message } => {
            writeln!(stdout, "update_result: update failed without restart")?;
            writeln!(stdout, "message: {message}")
        }
        UpdateResult::UpdatedAndHostRestarted { snapshot } => {
            writeln!(stdout, "update_result: updated and host restarted")?;
            render_snapshot(stdout, snapshot)
        }
        UpdateResult::UpdatedAndAppServerRestarted { snapshot } => {
            writeln!(stdout, "update_result: updated and app-server restarted")?;
            render_snapshot(stdout, snapshot)
        }
        UpdateResult::UpdatedButReplacementFailed {
            message,
            recovery_action,
        } => {
            writeln!(stdout, "update_result: updated but replacement failed")?;
            writeln!(stdout, "message: {message}")?;
            writeln!(stdout, "recovery_action: {recovery_action}")
        }
    }
}

fn render_snapshot<W: Write>(stdout: &mut W, snapshot: &HostSnapshot) -> std::io::Result<()> {
    render_fleet_status(stdout, snapshot.fleet())?;
    if let Some(outcome) = snapshot.last_login_reload() {
        writeln!(stdout, "last_login_reload: {}", outcome.description())?;
    }
    writeln!(
        stdout,
        "readiness: {}",
        readiness_label(snapshot.hosted_readiness(), snapshot.fleet())
    )?;
    writeln!(stdout, "phase: {}", phase_label(snapshot.phase()))?;
    writeln!(stdout, "router: {}", router_label(snapshot.router()))?;
    if let Some(ports) = snapshot.port_selection() {
        for (name, selected) in [
            ("router_port", ports.router),
            ("mcp_port", ports.mcp),
            ("tailscale_https_port", ports.tailscale_https),
        ] {
            writeln!(
                stdout,
                "{name}: {} ({})",
                selected.port,
                selected.source.label()
            )?;
        }
    } else {
        writeln!(
            stdout,
            "ports: unavailable (Host did not report startup selections)"
        )?;
    }
    writeln!(
        stdout,
        "app_server: {}",
        app_server_label(snapshot.app_server())
    )?;
    writeln!(
        stdout,
        "remote_control: {}",
        remote_control_label(snapshot.remote_control())
    )?;
    if let Some(identity) = snapshot.remote_control_identity() {
        writeln!(stdout, "remote_server_name: {}", identity.server_name())?;
        writeln!(
            stdout,
            "remote_environment_id: {}",
            identity.environment_id().unwrap_or("unassigned")
        )?;
    } else {
        writeln!(stdout, "remote_server_name: unavailable")?;
        writeln!(stdout, "remote_environment_id: unavailable")?;
    }
    writeln!(stdout, "desktop attachment: configured")?;
    writeln!(
        stdout,
        "desktop relaunch: restart required if already running"
    )?;
    writeln!(
        stdout,
        "codex_executable_relation: {}",
        executable_relation_label(snapshot.executable_relation())
    )?;
    let router_relation = snapshot.router_executable_relation();
    writeln!(
        stdout,
        "router_executable_relation: {}",
        router_executable_relation_label(router_relation)
    )?;
    if let Some(warning) = collaboration_protocol::router_build_warning(router_relation) {
        writeln!(stdout, "{warning}")?;
    }
    writeln!(
        stdout,
        "recovery_budget: {}",
        recovery_budget_label(snapshot.recovery_budget())
    )?;
    writeln!(
        stdout,
        "last_lifecycle_outcome: {}",
        outcome_label(snapshot.last_lifecycle_outcome())
    )
}

fn render_fleet_status<W: Write>(
    stdout: &mut W,
    observation: &codex_router_host::FleetHostObservation,
) -> std::io::Result<()> {
    use codex_router_host::FleetHostObservation;
    use collaboration_protocol::FleetLinkState;
    match observation {
        FleetHostObservation::NotObserved => writeln!(stdout, "central: not observed"),
        FleetHostObservation::Unavailable { message } => {
            writeln!(stdout, "central: unavailable ({message})")
        }
        FleetHostObservation::Available {
            central,
            stuck_mail,
        } => {
            match central {
                None => writeln!(stdout, "central: not configured")?,
                Some(link) => {
                    match &link.state {
                        FleetLinkState::Disconnected { since } => {
                            write!(stdout, "central: disconnected since {since}")
                        }
                        FleetLinkState::Connecting { since, attempts } => {
                            write!(
                                stdout,
                                "central: connecting since {since}; attempts {attempts}"
                            )
                        }
                        FleetLinkState::Connected {
                            since,
                            last_contact,
                        } => write!(
                            stdout,
                            "central: connected since {since}; last message from central {last_contact}"
                        ),
                        FleetLinkState::Refused { reason, at } => {
                            write!(stdout, "central: refused {reason:?} at {at}")
                        }
                    }?;
                    if let Some(drop) = &link.last_drop {
                        write!(
                            stdout,
                            "; last drop {} at {}",
                            drop.reason.as_str(),
                            drop.at
                        )?;
                    }
                    if let Some(failure) = &link.last_failure {
                        let reason = serde_json::to_string(&failure.reason)
                            .map_err(std::io::Error::other)?;
                        write!(stdout, "; last failure {reason} at {}", failure.at)?;
                    }
                    write!(
                        stdout,
                        "; instance {}; connection {}; revision {}",
                        link.instance, link.connection, link.revision
                    )?;
                    if !matches!(link.state, FleetLinkState::Connected { .. })
                        && let Some(last_frame_at) = link.last_frame_at
                    {
                        write!(stdout, "; last message from central {last_frame_at}")?;
                    }
                    writeln!(stdout)?;
                    writeln!(
                        stdout,
                        "central peer: {}; endpoint: {}",
                        link.peer, link.endpoint
                    )?;
                }
            }
            match stuck_mail {
                collaboration_protocol::StuckMailSummary::Unavailable { message } => {
                    writeln!(stdout, "outbox: unavailable ({message})")
                }
                collaboration_protocol::StuckMailSummary::AgeUnknown {
                    recovered_count,
                    known_stuck,
                } => {
                    if let Some(known) = known_stuck {
                        write!(
                            stdout,
                            "outbox: {} items sent longer than 5 minutes; oldest age {}s; ",
                            known.count, known.oldest_age_seconds
                        )?;
                    } else {
                        write!(stdout, "outbox: ")?;
                    }
                    writeln!(
                        stdout,
                        "{recovered_count} outstanding sent items; age unknown (recovered)"
                    )
                }
                collaboration_protocol::StuckMailSummary::Empty => {
                    writeln!(stdout, "outbox: 0 items sent longer than 5 minutes")
                }
                collaboration_protocol::StuckMailSummary::Stuck {
                    count,
                    oldest_age_seconds,
                } => writeln!(
                    stdout,
                    "outbox: {count} items sent longer than 5 minutes; oldest age {oldest_age_seconds}s"
                ),
            }
        }
    }
}

fn classification_label(
    classification: codex_router_host::TerminalClassification,
    fleet: &codex_router_host::FleetHostObservation,
) -> String {
    match classification {
        codex_router_host::TerminalClassification::Ready => {
            readiness_label(codex_router_host::HostedReadiness::Ready, fleet)
        }
        codex_router_host::TerminalClassification::LocalReadyRemoteDegraded => readiness_label(
            codex_router_host::HostedReadiness::LocalReadyRemoteDegraded,
            fleet,
        ),
        codex_router_host::TerminalClassification::Unavailable => "unavailable".to_owned(),
        codex_router_host::TerminalClassification::Succeeded => "succeeded".to_owned(),
        codex_router_host::TerminalClassification::Failed => "failed".to_owned(),
        codex_router_host::TerminalClassification::Busy => "busy".to_owned(),
    }
}

fn readiness_label(
    readiness: codex_router_host::HostedReadiness,
    fleet: &codex_router_host::FleetHostObservation,
) -> String {
    use codex_router_host::{FleetHostObservation, HostedReadiness};
    use collaboration_protocol::FleetLinkState;
    if let FleetHostObservation::Available {
        central: Some(record),
        ..
    } = fleet
        && let FleetLinkState::Refused { reason, .. } = &record.state
    {
        match readiness {
            HostedReadiness::Ready => return format!("local ready (central refused: {reason:?})"),
            HostedReadiness::LocalReadyRemoteDegraded => {
                return format!(
                    "local ready (Remote Control degraded, central refused: {reason:?})"
                );
            }
            HostedReadiness::Unavailable => {}
        }
    }
    match readiness {
        HostedReadiness::Ready => "ready".to_owned(),
        HostedReadiness::LocalReadyRemoteDegraded => {
            "local ready (Remote Control degraded)".to_owned()
        }
        HostedReadiness::Unavailable => "unavailable".to_owned(),
    }
}

fn phase_label(phase: &codex_router_host::HostPhase) -> &'static str {
    match phase {
        codex_router_host::HostPhase::Starting => "starting",
        codex_router_host::HostPhase::Steady => "steady",
        codex_router_host::HostPhase::Mutating { .. } => "mutating",
        codex_router_host::HostPhase::Stopping => "stopping",
    }
}

fn router_label(router: codex_router_host::RouterCondition) -> &'static str {
    match router {
        codex_router_host::RouterCondition::ExternalReachable => "external router ready",
        codex_router_host::RouterCondition::OwnedReachable => "host-owned router ready",
        codex_router_host::RouterCondition::OwnedTransitioning => "host-owned router starting",
        codex_router_host::RouterCondition::Unavailable => "unavailable",
    }
}

fn app_server_label(app_server: &codex_router_host::AppServerCondition) -> String {
    match app_server {
        codex_router_host::AppServerCondition::NativeReady { running_version } => {
            format!("ready ({running_version})")
        }
        codex_router_host::AppServerCondition::Starting => "starting".to_owned(),
        codex_router_host::AppServerCondition::Stopping => "stopping".to_owned(),
        codex_router_host::AppServerCondition::ShutdownTimedOut => "shutdown timed out".to_owned(),
        codex_router_host::AppServerCondition::Absent => "absent".to_owned(),
        codex_router_host::AppServerCondition::Failed => "failed".to_owned(),
    }
}

fn remote_control_label(condition: codex_router_host::RemoteControlCondition) -> &'static str {
    match condition {
        codex_router_host::RemoteControlCondition::Connected => "connected",
        codex_router_host::RemoteControlCondition::Connecting => "connecting",
        codex_router_host::RemoteControlCondition::Errored => "error",
        codex_router_host::RemoteControlCondition::Disabled => "disabled",
        codex_router_host::RemoteControlCondition::Unavailable => "unavailable",
    }
}

fn executable_relation_label(relation: codex_router_host::ExecutableRelation) -> &'static str {
    match relation {
        codex_router_host::ExecutableRelation::Match => "matches installed executable",
        codex_router_host::ExecutableRelation::Drift => "differs from installed executable",
        codex_router_host::ExecutableRelation::Unknown => "unknown",
    }
}

fn router_executable_relation_label(
    relation: &codex_router_host::RouterExecutableRelation,
) -> &'static str {
    match relation {
        codex_router_host::RouterExecutableRelation::Match => "match",
        codex_router_host::RouterExecutableRelation::Drift { .. } => "drift",
        codex_router_host::RouterExecutableRelation::Unknown { .. } => "unknown",
    }
}

fn recovery_budget_label(budget: codex_router_host::RecoveryBudget) -> &'static str {
    match budget {
        codex_router_host::RecoveryBudget::Available => "available",
        codex_router_host::RecoveryBudget::Consumed => "consumed",
    }
}

fn outcome_label(outcome: Option<&codex_router_host::LifecycleOutcome>) -> &'static str {
    let Some(outcome) = outcome else {
        return "none";
    };
    match outcome.classification {
        codex_router_host::LifecycleOutcomeClassification::Succeeded => "succeeded",
        codex_router_host::LifecycleOutcomeClassification::Failed => "failed",
        codex_router_host::LifecycleOutcomeClassification::Forced => "forced",
        codex_router_host::LifecycleOutcomeClassification::TimedOut => "timed out",
        codex_router_host::LifecycleOutcomeClassification::Busy => "busy",
    }
}

#[cfg(test)]
#[path = "host_test.rs"]
mod tests;
