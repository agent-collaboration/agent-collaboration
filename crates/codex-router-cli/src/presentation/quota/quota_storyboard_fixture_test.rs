//! Opt-in actual terminal fixture. Only external effects and records are supplied;
//! the component, input ownership, layout and state transitions remain real.
use super::*;
use crate::presentation::quota::quota_account_options::{
    CreditUsagePolicySaveError, CreditUsagePolicySaver, CreditUsageRefreshError,
    CreditUsageRefresher, credit_usage_compact_summary,
};
use crate::presentation::quota::quota_floor_editor::WeeklyQuotaFloorSaver;
use crate::quota_reset::reset_session_supervisor::{
    ConfirmationSelection, KnownConsumeOutcome, LiveWeeklyDisplayFacts, ResetCreditDisplayRecord,
    ResetCreditDisplayStatusDto, ResetEligibilityDisabledReason, ResetSessionIntent,
    ResetSessionPorts, ResetValueProvenance, ResetWorkflowSnapshot, TestConsumeUnknownReason,
    WorkflowPhase, WorkflowResult,
};
use crate::quota_reset::reset_workflow_reducer::WorkflowActivities;
use codex_router_core::credit_usage::{
    CreditAvailability, CreditBalance, CreditProviderObservation, CreditSpendControl,
};
use std::{
    io::{IsTerminal as _, Write as _},
    sync::Mutex,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FixtureScenario {
    Normal,
    VisualHardCases,
    InspectionPending,
    CommitPending,
    UnknownResult,
    SaveFailed,
    DisabledYes,
    RevalidationPending,
    RefusedResult,
    RefreshPending,
    SavePending,
}
impl FixtureScenario {
    fn from_environment() -> std::io::Result<Self> {
        match std::env::var("QUOTA_COMPONENT_FIXTURE").as_deref() {
            Ok("normal") => Ok(Self::Normal),
            Ok("visual-hard-cases") => Ok(Self::VisualHardCases),
            Ok("inspection-pending") => Ok(Self::InspectionPending),
            Ok("commit-pending") => Ok(Self::CommitPending),
            Ok("unknown-result") => Ok(Self::UnknownResult),
            Ok("save-failed") => Ok(Self::SaveFailed),
            Ok("disabled-yes") => Ok(Self::DisabledYes),
            Ok("revalidation-pending") => Ok(Self::RevalidationPending),
            Ok("refused-result") => Ok(Self::RefusedResult),
            Ok("refresh-pending") => Ok(Self::RefreshPending),
            Ok("save-pending") => Ok(Self::SavePending),
            _ => Err(std::io::Error::other(
                "explicit QUOTA_COMPONENT_FIXTURE scenario is required",
            )),
        }
    }
}

pub(super) fn fixture_model() -> QuotaStatusViewModel {
    let mut model = quota_two_account_view_model();
    model.header.observed_clients = Some(1);
    model.rows[0].account = "東京 OpenAI Research Account Alpha Long Name".to_owned();
    model.rows[0].details.account = model.rows[0].account.clone();
    model.rows[0].credit_usage.provider_observation = CreditProviderObservation::new(
        CreditAvailability::Available {
            balance: Some(CreditBalance::new("62500.0000000000000000").unwrap()),
        },
        CreditSpendControl::Clear,
        None,
    );
    model.rows[0].credit_usage.freshness = crate::quota::CreditUsageFreshness::Fresh;
    // Declared synthetic age, independent of the host clock or a provider request.
    model.rows[0].credit_usage.age_label = "0s".to_owned();
    model.rows[0].credit_usage_summary = credit_usage_compact_summary(&model.rows[0].credit_usage);
    model.rows[1].account = "Claude Research Account Beta Full Name".to_owned();
    model.rows[1].provider = codex_router_core::provider::Provider::Claude;
    model.rows[1].details.account = model.rows[1].account.clone();
    model
}

pub(super) fn fixture_scenario_model(scenario: FixtureScenario) -> QuotaStatusViewModel {
    let mut model = fixture_model();
    if scenario != FixtureScenario::VisualHardCases {
        return model;
    }
    use super::super::quota_status_view_model::QuotaDisplayWindow;
    model.rows[0].display_weekly = QuotaDisplayWindow::Known {
        remaining_percent: 92,
        reset_in_seconds: Some(576000),
        stale: false,
    };
    model.rows[0].display_short = QuotaDisplayWindow::Known {
        remaining_percent: 83,
        reset_in_seconds: Some(7800),
        stale: false,
    };
    model.rows[1].display_weekly = QuotaDisplayWindow::Known {
        remaining_percent: 75,
        reset_in_seconds: None,
        stale: true,
    };
    model.rows[1].display_short = QuotaDisplayWindow::Unknown;
    // Keep the hard-case Details snapshot consistent with its browse evidence.
    model.rows[0].details.weekly_window = "week ██████████ 92% left, reset 6d 16h".to_owned();
    model.rows[0].details.short_window = "5h █████████░ 83% left, reset 2h 10m".to_owned();
    model.rows[1].details.weekly_window = "week ████████░░ 75% left, reset unknown".to_owned();
    model.rows[1].details.short_window = "5h ░░░░░░░░░░ no data".to_owned();
    model.rows[1].credit_usage.provider_observation = CreditProviderObservation::new(
        CreditAvailability::Available {
            balance: Some(CreditBalance::new("125.50").unwrap()),
        },
        CreditSpendControl::Clear,
        None,
    );
    model.rows[1].credit_usage.freshness = crate::quota::CreditUsageFreshness::Stale;
    model.rows[1].credit_usage_summary = credit_usage_compact_summary(&model.rows[1].credit_usage);
    model.header.pool = super::super::quota_status_symbols::QuotaPoolCensus {
        usable: 1,
        reserve: 0,
        blocked: 1,
        unknown: 0,
        excluded: 0,
    };
    model.header.freshness = super::super::quota_status_symbols::QuotaFreshnessCensus {
        fresh: 0,
        stale: 1,
        unknown: 1,
    };
    model.header.credential_notice = Some("Credentials locked".to_owned());
    for row in &mut model.rows {
        row.status = super::super::quota_status_symbols::QuotaRowStatus::CredentialUnavailable {
            notice: "Credentials locked".to_owned(),
        };
        row.details.status = "Credentials locked".to_owned();
    }
    model
}

pub(super) fn fixture_snapshot(
    phase: WorkflowPhase,
    selection: ConfirmationSelection,
    scenario: FixtureScenario,
) -> ResetWorkflowSnapshot {
    let result = (phase == WorkflowPhase::Result).then(|| {
        if scenario == FixtureScenario::UnknownResult {
            WorkflowResult::OutcomeUnknown(TestConsumeUnknownReason::Transport)
        } else {
            if scenario == FixtureScenario::RefusedResult {
                WorkflowResult::Refused(
                    crate::quota_reset::reset_session_supervisor::TestRenderSafeFailure::Transport,
                )
            } else {
                WorkflowResult::Known(KnownConsumeOutcome::Reset { windows_reset: 1 })
            }
        }
    });
    let credits = (1..=7)
        .map(|index| ResetCreditDisplayRecord {
            id_hint: format!("voucher-{index}"),
            status: ResetCreditDisplayStatusDto::Available,
            title: Some(format!("Synthetic reset voucher {index}")),
            expires_unix_seconds: None,
            earliest_usable: index == 1,
        })
        .collect();
    ResetWorkflowSnapshot::test_snapshot(
        phase,
        selection,
        WorkflowActivities::default(),
        result,
        Some(LiveWeeklyDisplayFacts {
            remaining_percent: 5,
            provenance: ResetValueProvenance::CurrentLive,
        }),
        credits,
        (scenario == FixtureScenario::DisabledYes)
            .then_some(ResetEligibilityDisabledReason::NoUsableCredit),
    )
}

pub(super) fn fixture_ports(
    model: Arc<Mutex<QuotaStatusViewModel>>,
    scenario: FixtureScenario,
    release: tokio::sync::watch::Receiver<bool>,
) -> (
    WeeklyQuotaFloorSaver,
    CreditUsagePolicySaver,
    CreditUsageRefresher,
) {
    let floor_model = model.clone();
    let floor: WeeklyQuotaFloorSaver = Arc::new(move |account_id, percent| {
        let mut model = floor_model.lock().unwrap();
        let row = model
            .rows
            .iter_mut()
            .find(|row| row.account_id == account_id)
            .expect("captured fixture account");
        row.weekly_quota_floor_percent = percent;
        Box::pin(async { Ok(()) })
    });
    let policy_model = model.clone();
    let policy_release = release.clone();
    let policy: CreditUsagePolicySaver = Arc::new(move |account_id, generation, policy| {
        let mut model = policy_model.lock().unwrap();
        let row = model
            .rows
            .iter_mut()
            .find(|row| row.account_id == account_id)
            .expect("captured fixture account");
        assert_eq!(row.active_credential_generation, generation);
        if scenario == FixtureScenario::SavePending {
            let mut release = policy_release.clone();
            let pending_model = policy_model.clone();
            return Box::pin(async move {
                release
                    .wait_for(|released| *released)
                    .await
                    .map_err(|_| CreditUsagePolicySaveError::StateOperationFailed)?;
                let mut model = pending_model.lock().unwrap();
                let row = model
                    .rows
                    .iter_mut()
                    .find(|row| row.account_id == account_id)
                    .expect("captured fixture account after settlement");
                assert_eq!(row.active_credential_generation, generation);
                row.credit_usage.policy = policy;
                Ok(policy)
            });
        }
        if scenario != FixtureScenario::SaveFailed {
            row.credit_usage.policy = policy;
        }
        Box::pin(async move {
            if scenario == FixtureScenario::SaveFailed {
                Err(CreditUsagePolicySaveError::StateOperationFailed)
            } else {
                Ok(policy)
            }
        })
    });
    let refresh: CreditUsageRefresher = Arc::new(move |account_id, generation| {
        let mut model = model.lock().unwrap();
        let row = model
            .rows
            .iter_mut()
            .find(|row| row.account_id == account_id)
            .expect("captured fixture account");
        assert_eq!(row.active_credential_generation, generation);
        if scenario == FixtureScenario::RefreshPending {
            let mut release = release.clone();
            return Box::pin(async move {
                release
                    .wait_for(|released| *released)
                    .await
                    .map_err(|_| CreditUsageRefreshError::Failed)?;
                Ok(())
            });
        }
        if scenario == FixtureScenario::Normal {
            // A distinct response lets the UI journey reject a stale reload.
            row.credit_usage.provider_observation = CreditProviderObservation::new(
                CreditAvailability::Available {
                    balance: Some(CreditBalance::new("63000.0000000000000000").unwrap()),
                },
                CreditSpendControl::Clear,
                None,
            );
            row.credit_usage.freshness = crate::quota::CreditUsageFreshness::Fresh;
            row.credit_usage.age_label = "0s".to_owned();
            row.credit_usage_summary = credit_usage_compact_summary(&row.credit_usage);
        }
        Box::pin(async { Ok(()) })
    });
    (floor, policy, refresh)
}

fn fixture_release_path(scenario: FixtureScenario) -> std::io::Result<Option<std::path::PathBuf>> {
    if !matches!(
        scenario,
        FixtureScenario::InspectionPending
            | FixtureScenario::CommitPending
            | FixtureScenario::RevalidationPending
            | FixtureScenario::RefreshPending
            | FixtureScenario::SavePending
    ) {
        return Ok(None);
    }
    let path = std::path::PathBuf::from(
        std::env::var_os("QUOTA_COMPONENT_FIXTURE_RELEASE_FILE").ok_or_else(|| {
            std::io::Error::other("pending fixture requires an owned release file path")
        })?,
    );
    if !path.is_absolute() || std::fs::symlink_metadata(&path).is_ok() {
        return Err(std::io::Error::other(
            "release file must be absolute and absent at fixture start",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("release file needs an existing owned parent"))?;
    if !parent.is_dir() {
        return Err(std::io::Error::other("release parent must already exist"));
    }
    Ok(Some(path))
}

async fn wait_for_fixture_release(path: std::path::PathBuf) -> std::io::Result<()> {
    tokio::time::timeout(Duration::from_secs(600), async move {
        let mut poll = tokio::time::interval(Duration::from_millis(40));
        loop {
            poll.tick().await;
            match tokio::fs::read(&path).await {
                Ok(bytes) if bytes == b"settle\n" => return Ok(()),
                Ok(_) => {
                    return Err(std::io::Error::other(
                        "fixture release must contain exact settle newline",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    })
    .await
    .map_err(|_| std::io::Error::other("fixture settlement deadline elapsed"))?
}

fn fixture_receipt_path(
    value: Option<std::ffi::OsString>,
) -> std::io::Result<Option<std::path::PathBuf>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let path = std::path::PathBuf::from(value);
    if !path.is_absolute()
        || std::fs::symlink_metadata(&path).is_ok()
        || !path.parent().is_some_and(std::path::Path::is_dir)
    {
        return Err(std::io::Error::other(
            "fixture receipt must be an absolute absent file in an existing owned output directory",
        ));
    }
    Ok(Some(path))
}

fn write_fixture_receipt(path: Option<&std::path::Path>, receipt: &str) -> std::io::Result<()> {
    if let Some(path) = path {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(receipt.as_bytes())?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "actual PTY fixture requires explicit scenario and exclusive declared capture target"]
async fn compiled_quota_component_storyboard_fixture() -> std::io::Result<()> {
    let scenario = FixtureScenario::from_environment()?;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(std::io::Error::other("fixture requires its owned real PTY"));
    }
    let model = Arc::new(Mutex::new(fixture_scenario_model(scenario)));
    let release_path = fixture_release_path(scenario)?;
    let receipt_path =
        fixture_receipt_path(std::env::var_os("QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE"))?;
    let (release_sender, release_receiver) = tokio::sync::watch::channel(false);
    let (floor, policy, refresh) = fixture_ports(model.clone(), scenario, release_receiver);
    let loader: QuotaStatusViewModelLoader = {
        let model = model.clone();
        Arc::new(move || {
            let snapshot = model.lock().unwrap().clone();
            Box::pin(async move { Some(snapshot) })
        })
    };
    let initial = fixture_snapshot(WorkflowPhase::Browse, ConfirmationSelection::No, scenario);
    let (ports, mut intents, snapshots) = ResetSessionPorts::test_channels(initial);
    let release_task = release_path.map(|path| {
        let release_sender = release_sender.clone();
        let snapshots = snapshots.clone();
        tokio::spawn(async move {
            wait_for_fixture_release(path).await?;
            let next = match scenario {
                FixtureScenario::CommitPending | FixtureScenario::RevalidationPending => {
                    let expected = if scenario == FixtureScenario::CommitPending {
                        WorkflowPhase::Committing
                    } else {
                        WorkflowPhase::Revalidating
                    };
                    if snapshots.borrow().phase() != expected {
                        return Err(std::io::Error::other(
                            "settlement requested before pending reset was observed",
                        ));
                    }
                    Some(fixture_snapshot(
                        WorkflowPhase::Result,
                        ConfirmationSelection::Yes,
                        scenario,
                    ))
                }
                FixtureScenario::InspectionPending => {
                    if snapshots.borrow().phase() != WorkflowPhase::Inspecting {
                        return Err(std::io::Error::other(
                            "settlement requested before inspection was pending",
                        ));
                    }
                    Some(fixture_snapshot(
                        WorkflowPhase::Inspected,
                        ConfirmationSelection::No,
                        scenario,
                    ))
                }
                _ => None,
            };
            if let Some(next) = next {
                snapshots.send(next).map_err(|_| {
                    std::io::Error::other("fixture snapshot receiver closed before settlement")
                })?;
            }
            release_sender.send_replace(true);
            Ok::<(), std::io::Error>(())
        })
    });
    let expected_accounts = model
        .lock()
        .unwrap()
        .rows
        .iter()
        .map(|row| (row.account_id.clone(), row.active_credential_generation))
        .collect::<Vec<_>>();
    let driver = tokio::spawn(async move {
        let mut selection = ConfirmationSelection::No;
        while let Some(intent) = intents.recv().await {
            let phase = match intent {
                ResetSessionIntent::BeginInspection {
                    account_id,
                    active_credential_generation,
                    ..
                } => {
                    assert!(
                        expected_accounts
                            .contains(&(account_id, Some(active_credential_generation)))
                    );
                    selection = ConfirmationSelection::No;
                    if scenario == FixtureScenario::InspectionPending {
                        WorkflowPhase::Inspecting
                    } else {
                        WorkflowPhase::Inspected
                    }
                }
                ResetSessionIntent::CancelInspectionForTab { request_id, .. } => {
                    let next = fixture_snapshot(WorkflowPhase::Browse, selection, scenario)
                        .with_last_processed_inspection_tab_request_for_test(request_id);
                    if snapshots.send(next).is_err() {
                        break;
                    }
                    continue;
                }
                ResetSessionIntent::OpenConfirmation => WorkflowPhase::Confirming,
                ResetSessionIntent::SelectNo => {
                    selection = ConfirmationSelection::No;
                    WorkflowPhase::Confirming
                }
                ResetSessionIntent::SelectYes => {
                    assert_ne!(scenario, FixtureScenario::DisabledYes);
                    selection = ConfirmationSelection::Yes;
                    WorkflowPhase::Confirming
                }
                ResetSessionIntent::Confirm { .. } => {
                    if selection == ConfirmationSelection::No {
                        WorkflowPhase::Inspected
                    } else if scenario == FixtureScenario::CommitPending {
                        WorkflowPhase::Committing
                    } else if scenario == FixtureScenario::RevalidationPending {
                        WorkflowPhase::Revalidating
                    } else {
                        WorkflowPhase::Result
                    }
                }
                ResetSessionIntent::Cancel | ResetSessionIntent::DismissResult => {
                    WorkflowPhase::Browse
                }
                ResetSessionIntent::Shutdown => break,
                ResetSessionIntent::PinnedTargetInvalidated { .. } => {
                    panic!("fixture identity changed unexpectedly")
                }
            };
            if snapshots
                .send(fixture_snapshot(phase, selection, scenario))
                .is_err()
            {
                break;
            }
        }
    });
    let view_model = model.lock().unwrap().clone();
    let viewport_trace = receipt_path
        .as_ref()
        .map(|_| super::super::quota_viewport_trace::QuotaViewportTrace::default());
    let result=element!(QuotaStatusComponent(view_model,viewport_trace:viewport_trace.clone(),width:0usize,height:0usize,reload_view_model:Some(loader),weekly_floor_saver:Some(floor),credit_policy_saver:Some(policy),credit_usage_refresher:Some(refresh),reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)))
        .render_loop().fullscreen().ignore_ctrl_c().await;
    driver.abort();
    if let Err(error) = driver.await
        && !error.is_cancelled()
    {
        return Err(std::io::Error::other(format!(
            "fixture intent driver failed: {error}"
        )));
    }
    if let Some(task) = release_task {
        task.abort();
        match task.await {
            Ok(result) => result?,
            Err(error) if error.is_cancelled() => {}
            Err(error) => {
                return Err(std::io::Error::other(format!(
                    "fixture settlement task failed: {error}"
                )));
            }
        }
    }
    let released = *release_sender.borrow();
    if scenario == FixtureScenario::CommitPending && !released {
        return Err(std::io::Error::other(
            "committing fixture exited before explicit settlement",
        ));
    }
    result?;
    let receipt = format!(
        "QUOTA_COMPONENT_FIXTURE_FINISHED scenario={scenario:?} released={released} external_requests=0 persisted_writes=0\n"
    );
    if let (Some(trace), Some(path)) = (viewport_trace, receipt_path.as_deref()) {
        trace.write_next_to(path)?;
    }
    write_fixture_receipt(receipt_path.as_deref(), &receipt)?;
    print!("{receipt}");
    Ok(())
}

#[test]
fn storyboard_fixture_retains_exact_balance_and_each_declared_reset_state() {
    let model = fixture_model();
    assert_eq!(model.rows[0].credit_usage_summary, "62.5k fresh");
    assert_eq!(model.rows[0].credit_usage.age_label, "0s");
    assert_eq!(
        model.rows[0]
            .credit_usage
            .provider_observation
            .availability(),
        &CreditAvailability::Available {
            balance: Some(CreditBalance::new("62500.0000000000000000").unwrap())
        }
    );
    for phase in [
        WorkflowPhase::Inspecting,
        WorkflowPhase::Inspected,
        WorkflowPhase::Confirming,
        WorkflowPhase::Committing,
        WorkflowPhase::Result,
    ] {
        let snapshot = fixture_snapshot(phase, ConfirmationSelection::No, FixtureScenario::Normal);
        assert_eq!(snapshot.phase(), phase);
        assert_eq!(snapshot.credit_inventory().len(), 7);
    }
    assert!(
        !fixture_snapshot(
            WorkflowPhase::Confirming,
            ConfirmationSelection::No,
            FixtureScenario::DisabledYes
        )
        .yes_enabled()
    );
    assert!(
        fixture_snapshot(
            WorkflowPhase::Confirming,
            ConfirmationSelection::No,
            FixtureScenario::Normal
        )
        .yes_enabled()
    );
}

#[tokio::test]
async fn pending_fixture_policy_settles_only_after_explicit_release() {
    let model = Arc::new(Mutex::new(fixture_model()));
    let row = model.lock().unwrap().rows[0].clone();
    let (sender, receiver) = tokio::sync::watch::channel(false);
    let (_, saver, _) = fixture_ports(model.clone(), FixtureScenario::SavePending, receiver);
    let mut pending = saver(
        row.account_id.clone(),
        row.active_credential_generation,
        codex_router_core::credit_usage::CreditUsagePolicy::Allow,
    );
    assert!(
        futures_util::poll!(pending.as_mut()).is_pending(),
        "pending save must remain held before release"
    );
    sender.send_replace(true);
    let saved = tokio::time::timeout(Duration::from_secs(1), pending)
        .await
        .expect("explicit fixture release must settle the pending effect")
        .unwrap();
    assert_eq!(
        saved,
        codex_router_core::credit_usage::CreditUsagePolicy::Allow
    );
    assert_eq!(model.lock().unwrap().rows[0].credit_usage.policy, saved);
}

#[tokio::test]
async fn release_file_accepts_only_explicit_settlement_and_rejects_wrong_payload() {
    for (payload, expected) in [
        (b"settle\n".as_slice(), true),
        (b"wrong\n".as_slice(), false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settlement");
        tokio::fs::write(&path, payload).await.unwrap();
        assert_eq!(wait_for_fixture_release(path).await.is_ok(), expected);
    }
}

#[test]
fn fixture_receipt_requires_fresh_output_and_never_overwrites_a_previous_result() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("receipt");
    assert!(fixture_receipt_path(Some(std::ffi::OsString::from("relative-receipt"))).is_err());
    assert!(
        fixture_receipt_path(Some(
            root.path().join("missing-parent/receipt").into_os_string()
        ))
        .is_err()
    );
    let resolved = fixture_receipt_path(Some(path.clone().into_os_string())).unwrap();
    write_fixture_receipt(resolved.as_deref(), "completed\n").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"completed\n");
    assert!(fixture_receipt_path(Some(path.clone().into_os_string())).is_err());
    assert_eq!(
        write_fixture_receipt(Some(&path), "replacement\n")
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(path).unwrap(), b"completed\n");
}

#[path = "quota_storyboard_observation_test.rs"]
mod quota_storyboard_observation_test;

#[test]
fn visual_hard_case_fixture_has_attributable_unknown_stale_and_credential_facts() {
    use super::super::quota_status_view_model::QuotaDisplayWindow;
    let model = fixture_scenario_model(FixtureScenario::VisualHardCases);
    assert_eq!(
        model.rows[0].display_weekly,
        QuotaDisplayWindow::Known {
            remaining_percent: 92,
            reset_in_seconds: Some(576000),
            stale: false
        }
    );
    assert_eq!(model.rows[1].display_short, QuotaDisplayWindow::Unknown);
    assert_eq!(
        model.rows[1].display_weekly,
        QuotaDisplayWindow::Known {
            remaining_percent: 75,
            reset_in_seconds: None,
            stale: true
        }
    );
    assert_eq!(model.rows[1].credit_usage_summary, "125.5 stale");
    assert_eq!(model.header.pool.reserve, 0);
    assert_eq!(
        model.header.credential_notice.as_deref(),
        Some("Credentials locked")
    );
}
