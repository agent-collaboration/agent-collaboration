//! Closed fixture commands and answers; observations leave only as returned IPC.
use super::launcher_failure::{LauncherFailure, LauncherResult};
use collaboration_central_core::signin_test_observation::SignInObservation;
use collaboration_central_tokio::TokioWorkspaceHost;
use fleet_identity_protocol::OrganizationId;
use router_clock::ManualClock;
use runtime_primitives::{Clock, SyncTime};
use serde::{Deserialize, Serialize};
use std::{io::Write, sync::Arc};

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum LauncherCommand {
    #[serde(rename_all = "camelCase")]
    SetTime {
        set_time_ms: i64,
    },
    Observe {
        observe: OrganizationId,
    },
}

#[derive(Serialize)]
#[serde(
    tag = "answer",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(super) enum LauncherAnswer {
    TimeSet {
        set_time_ms: i64,
        live_fleets: usize,
    },
    Observed {
        organization: OrganizationId,
        observation: Box<SignInObservation>,
    },
    Refused {
        error: &'static str,
    },
    Failed {
        error: &'static str,
    },
}

pub(super) async fn answer(
    host: &TokioWorkspaceHost,
    clock: &Arc<ManualClock>,
    line: &[u8],
) -> LauncherAnswer {
    let command: LauncherCommand = match serde_json::from_slice(line) {
        Ok(command) => command,
        Err(_) => {
            return LauncherAnswer::Refused {
                error: "invalid launcher command",
            };
        }
    };
    match command {
        LauncherCommand::SetTime { set_time_ms } => {
            let moved = if set_time_ms >= clock.now().unix_millis() {
                clock
                    .advance_to(SyncTime::from_unix_millis(set_time_ms))
                    .is_ok()
            } else if let Some(utc) = chrono::DateTime::from_timestamp_millis(set_time_ms) {
                clock.set_utc(utc);
                true
            } else {
                false
            };
            if !moved {
                return LauncherAnswer::Failed {
                    error: "launcher time is outside the supported range",
                };
            }
            let live_fleets = host.signin_live_organizations().await.len();
            LauncherAnswer::TimeSet {
                set_time_ms,
                live_fleets,
            }
        }
        LauncherCommand::Observe { observe } => {
            match host.observe_signin_organization(&observe).await {
                Ok(observation) => LauncherAnswer::Observed {
                    organization: observe,
                    observation: Box::new(observation),
                },
                Err(_) => LauncherAnswer::Failed {
                    error: "organization observation unavailable",
                },
            }
        }
    }
}

pub(super) fn write_answer(answer: &LauncherAnswer) -> LauncherResult {
    let mut output = std::io::stderr().lock();
    serde_json::to_writer(&mut output, answer).map_err(|_| LauncherFailure::WriteAnswer)?;
    writeln!(output).map_err(|_| LauncherFailure::WriteAnswer)?;
    output.flush().map_err(|_| LauncherFailure::WriteAnswer)
}
