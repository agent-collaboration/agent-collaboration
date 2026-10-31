//! Explicit debug-fixture readbacks, derived after the real render loop returns.
use crate::session_picker::{
    PickerMachineSourceMode, PickerSourceContext, SessionsPickerOutcome, SessionsPickerRequest,
};
use agent_session_inventory::sessions::{
    SessionPickerIdentity, claude_terminal_launch::ClaudeTerminalMode,
    source_client_action::SessionClientEffect, source_ssh_protocol::SourceNativeClaudeAction,
};
use collaboration_client::protocol::SessionRef;
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum PickerIdentityReadback<'a> {
    LocalCodex {
        #[serde(rename = "sessionId")]
        session_id: &'a str,
    },
    HostedCodex {
        target: &'a SessionRef,
    },
    HostedProvider {
        target: &'a SessionRef,
    },
    NativeClaude {
        target: &'a SessionRef,
    },
}
impl<'a> From<&'a SessionPickerIdentity> for PickerIdentityReadback<'a> {
    fn from(identity: &'a SessionPickerIdentity) -> Self {
        match identity {
            SessionPickerIdentity::LocalCodex(session_id) => Self::LocalCodex { session_id },
            SessionPickerIdentity::HostedCodex(target) => Self::HostedCodex { target },
            SessionPickerIdentity::HostedProvider(target) => Self::HostedProvider { target },
            SessionPickerIdentity::NativeClaude(target) => Self::NativeClaude { target },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum FixtureRunMode {
    Foreground,
    BackgroundThenAttach,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NewSessionReadback {
    client: &'static str,
    machine: String,
    directory: PathBuf,
    run_mode: FixtureRunMode,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum PickerOutcomeReadback<'a> {
    Cancelled,
    Resume {
        identity: PickerIdentityReadback<'a>,
    },
    Fork {
        identity: PickerIdentityReadback<'a>,
    },
    New {
        request: NewSessionReadback,
    },
}

pub(super) fn receipt_directory() -> io::Result<Option<PathBuf>> {
    let directory = match std::env::var_os("TUI_SESSIONS_FIXTURE_RECEIPT_DIR") {
        None => return Ok(None),
        Some(directory) => PathBuf::from(directory),
    };
    if !directory.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "fixture receipt directory must be absolute",
        ));
    }
    let metadata = fs::symlink_metadata(&directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "fixture receipt directory must be an existing non-symlinked directory",
        ));
    }
    Ok(Some(directory))
}

fn machine_key(source: &PickerSourceContext) -> String {
    match source {
        PickerSourceContext::DefaultHosted => "defaultHosted".into(),
        PickerSourceContext::LocalCodex => "localCodex".into(),
        PickerSourceContext::ConfiguredHosted(profile) => {
            format!("configured:{}", profile.name.as_str())
        }
    }
}

fn outcome_readback<'a>(
    outcome: &'a Option<SessionsPickerOutcome>,
    request: &SessionsPickerRequest,
) -> io::Result<PickerOutcomeReadback<'a>> {
    match outcome {
        None => Ok(PickerOutcomeReadback::Cancelled),
        Some(SessionsPickerOutcome::ResumeSession(selection)) => {
            Ok(PickerOutcomeReadback::Resume {
                identity: (&selection.identity).into(),
            })
        }
        Some(SessionsPickerOutcome::ForkSession(selection)) => Ok(PickerOutcomeReadback::Fork {
            identity: (&selection.identity).into(),
        }),
        Some(SessionsPickerOutcome::StartNewSession) => Ok(PickerOutcomeReadback::New {
            request: NewSessionReadback {
                client: "codex",
                machine: match request.machine_mode {
                    PickerMachineSourceMode::HostedDefault => "defaultHosted",
                    PickerMachineSourceMode::LocalCodex => "localCodex",
                }
                .into(),
                directory: request.current_dir.clone(),
                run_mode: FixtureRunMode::Foreground,
            },
        }),
        Some(SessionsPickerOutcome::LaunchNativeClaude(captured, mode)) => {
            if let SessionClientEffect::Native { action, .. } = &captured.effect {
                match action {
                    SourceNativeClaudeAction::Resume { target, .. } => {
                        return Ok(PickerOutcomeReadback::Resume {
                            identity: PickerIdentityReadback::NativeClaude { target },
                        });
                    }
                    SourceNativeClaudeAction::Fork { target, .. } => {
                        return Ok(PickerOutcomeReadback::Fork {
                            identity: PickerIdentityReadback::NativeClaude { target },
                        });
                    }
                    SourceNativeClaudeAction::New { .. } => {}
                }
            }
            let SessionClientEffect::Native {
                action: SourceNativeClaudeAction::New { directory, .. },
                ..
            } = &captured.effect
            else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "native resume/fork terminal handoff is outside this fixture outcome case",
                ));
            };
            Ok(PickerOutcomeReadback::New {
                request: NewSessionReadback {
                    client: "claudeCode",
                    machine: machine_key(&captured.source),
                    directory: PathBuf::from(directory.as_str()),
                    run_mode: match mode {
                        ClaudeTerminalMode::Foreground => FixtureRunMode::Foreground,
                        ClaudeTerminalMode::BackgroundThenAttach => {
                            FixtureRunMode::BackgroundThenAttach
                        }
                    },
                },
            })
        }
        Some(SessionsPickerOutcome::PreparedSourceSession(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "prepared configured-source outcome is outside the local/U4 fixture outcome case",
        )),
        Some(SessionsPickerOutcome::InterruptedClientAction(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "interrupted client action is outside the local/U4 fixture outcome case",
        )),
        Some(SessionsPickerOutcome::TerminalTooNarrow) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal-too-narrow outcome is outside the local/U4 fixture outcome case",
        )),
    }
}

pub(super) fn write_outcome(
    directory: &Path,
    outcome: &Option<SessionsPickerOutcome>,
    request: &SessionsPickerRequest,
) -> io::Result<()> {
    let readback = outcome_readback(outcome, request)?;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("picker-outcome.json"))?;
    serde_json::to_writer(&file, &readback).map_err(io::Error::other)?;
    file.sync_all()
}

/// The configured-source fixture keeps the existing preparation port real and
/// replaces only its external SSH read. It never starts a client or a model.
pub(super) fn recording_preparer(
    directory: Option<&Path>,
) -> super::super::super::source_action_worker::SourceActionPreparer {
    use agent_session_inventory::sessions::source_ssh_protocol::{
        PreparedSourceLaunch, SourceReadError, SourceRejection, SourceSessionAction,
    };
    use std::{
        io::{Seek, Write},
        sync::{Arc, Mutex},
    };
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct PreparationReadback {
        call_count: usize,
        request: NewSessionReadback,
    }
    struct RecordingState {
        count: usize,
        file: Option<fs::File>,
    }
    let state = Arc::new(Mutex::new(RecordingState {
        count: 0,
        file: None,
    }));
    let receipt = directory.map(|directory| directory.join("source-preparation.json"));
    Arc::new(move |profile, action| {
        let state = state.clone();
        let receipt = receipt.clone();
        Box::pin(async move {
            let client_profile = match &profile.transport {
                agent_session_inventory::sessions::router_connection_registry::RouterConnectionTransport::Ssh(connection) => connection.client_profile,
                agent_session_inventory::sessions::router_connection_registry::RouterConnectionTransport::Remote { .. } => return Err(SourceReadError::Rejected(SourceRejection::InvalidRequest)),
            };
            let SourceSessionAction::New { directory } = &action else {
                return Err(SourceReadError::Rejected(SourceRejection::InvalidRequest));
            };
            if let Some(path) = receipt {
                let write = || -> io::Result<()> {
                    let mut state = state
                        .lock()
                        .map_err(|_| io::Error::other("fixture preparation receipt lock failed"))?;
                    state.count = state.count.saturating_add(1);
                    let readback = PreparationReadback {
                        call_count: state.count,
                        request: NewSessionReadback {
                            client: "codex",
                            machine: format!("configured:{}", profile.name.as_str()),
                            directory: PathBuf::from(directory.as_str()),
                            run_mode: FixtureRunMode::Foreground,
                        },
                    };
                    if state.file.is_none() {
                        state.file = Some(
                            fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(path)?,
                        );
                    }
                    let file = state.file.as_mut().ok_or_else(|| {
                        io::Error::other("fixture preparation receipt unavailable")
                    })?;
                    file.rewind()?;
                    file.set_len(0)?;
                    serde_json::to_writer(&mut *file, &readback).map_err(io::Error::other)?;
                    file.flush()?;
                    file.sync_all()
                };
                if write().is_err() {
                    return Err(SourceReadError::Rejected(
                        SourceRejection::SourceUnavailable,
                    ));
                }
            }
            let endpoint = collaboration_client::protocol::EndpointRef {
                service_id: profile.service_id.clone(),
                endpoint_id: "codex-local"
                    .to_owned()
                    .try_into()
                    .map_err(|_| SourceReadError::Rejected(SourceRejection::InvalidRequest))?,
            };
            let generation=serde_json::from_value(serde_json::json!({"serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":1})).map_err(|_|SourceReadError::Rejected(SourceRejection::InvalidRequest))?;
            let schema_digest = format!("sha256:{}", "a".repeat(64))
                .try_into()
                .map_err(|_| SourceReadError::Rejected(SourceRejection::InvalidRequest))?;
            Ok(PreparedSourceLaunch {
                action: action.clone(),
                endpoint,
                generation,
                schema_digest,
                working_directory: directory.clone(),
                client_profile,
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_picker::test_support::{picker_record, picker_request};
    use agent_session_inventory::sessions::SessionActionSelection;
    use serde_json::json;

    #[test]
    fn outcome_serialization_preserves_same_id_source_variants_and_the_actual_action() {
        let target:SessionRef=serde_json::from_value(json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"shared-opaque-id"})).unwrap();
        for (kind, identity) in [
            (
                "hostedCodex",
                SessionPickerIdentity::HostedCodex(target.clone()),
            ),
            (
                "hostedProvider",
                SessionPickerIdentity::HostedProvider(target.clone()),
            ),
            (
                "nativeClaude",
                SessionPickerIdentity::NativeClaude(target.clone()),
            ),
        ] {
            let mut record = picker_record(
                "shared-opaque-id",
                "Synthetic receipt",
                "/fixture/project",
                "openai",
                "interactive",
            );
            record.identity = identity;
            let selection = SessionActionSelection::from_picker_record(&record);
            for (action, outcome) in [
                (
                    "resume",
                    SessionsPickerOutcome::ResumeSession(selection.clone()),
                ),
                (
                    "fork",
                    SessionsPickerOutcome::ForkSession(selection.clone()),
                ),
            ] {
                let outcome = Some(outcome);
                let actual =
                    serde_json::to_value(outcome_readback(&outcome, &picker_request()).unwrap())
                        .unwrap();
                assert_eq!(
                    actual,
                    json!({"kind":action,"identity":{"kind":kind,"target":target}})
                );
            }
        }
    }

    #[test]
    fn returned_local_new_and_cancellation_have_distinct_readbacks() {
        let mut request = picker_request();
        request.current_dir = PathBuf::from("/fixture/project");
        assert_eq!(
            serde_json::to_value(
                outcome_readback(&Some(SessionsPickerOutcome::StartNewSession), &request).unwrap()
            )
            .unwrap(),
            json!({"kind":"new","request":{"client":"codex","machine":"defaultHosted","directory":"/fixture/project","runMode":"foreground"}})
        );
        assert_eq!(
            serde_json::to_value(outcome_readback(&None, &request).unwrap()).unwrap(),
            json!({"kind":"cancelled"})
        );
        assert!(
            outcome_readback(&Some(SessionsPickerOutcome::TerminalTooNarrow), &request).is_err()
        );
    }

    #[test]
    fn outcome_receipt_is_durable_and_never_overwrites_an_existing_run() {
        let directory = tempfile::tempdir().unwrap();
        write_outcome(directory.path(), &None, &picker_request()).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("picker-outcome.json")).unwrap(),
            "{\"kind\":\"cancelled\"}"
        );
        assert_eq!(
            write_outcome(
                directory.path(),
                &Some(SessionsPickerOutcome::StartNewSession),
                &picker_request()
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[tokio::test]
    async fn configured_new_records_the_actual_preparation_port_request_and_duplicate_count() {
        use agent_session_inventory::sessions::{
            RouterRegistryRead, source_ssh_protocol::SourceSessionAction,
        };
        let request =
            crate::session_picker::provider_filter_storyboard_fixture::apply_storyboard_variant(
                picker_request(),
                Some("configured-source-new"),
            )
            .unwrap();
        let RouterRegistryRead::Ready(registry) = request.router_registry else {
            panic!("configured fixture registry");
        };
        let profile = registry.routers.first().unwrap().clone();
        let action = SourceSessionAction::New {
            directory: profile.default_remote_cwd.clone().unwrap(),
        };
        let directory = tempfile::tempdir().unwrap();
        let preparer = recording_preparer(Some(directory.path()));
        let prepared = preparer(profile.clone(), action.clone()).await.unwrap();
        assert_eq!(prepared.action, action);
        let first: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(directory.path().join("source-preparation.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            first,
            serde_json::json!({"callCount":1,"request":{"client":"codex","machine":"configured:Fixture remote","directory":"/fixture/remote","runMode":"foreground"}})
        );
        preparer(profile, action).await.unwrap();
        let second: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(directory.path().join("source-preparation.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            second["callCount"], 2,
            "duplicate preparation must remain visible to the strict reader"
        );
    }
}

#[cfg(test)]
#[path = "picker_fixture_scenario_tests.rs"]
mod scenario_tests;
