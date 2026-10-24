//! L3 normal-prefix and unavailable-socket cases through the real local Runtime and SQLite.
use crate::owned_resource_cleanup::readback::CleanupReadback;
use crate::{
    owned_resource_cleanup::{
        CleanupAction, CleanupObservation, CleanupOutcome, MatrixCleanupReport,
        cleanup_owned_resources,
    },
    owned_resources::InstructionCreation,
    proof_context::ProofResult,
};
use automation_storage::AutomationStore;
use collaboration_client::{
    CollaborationClient,
    board::{EndReason, SubscriptionState, ThreadShowRequest},
    protocol::{
        EndpointRef, SavedMessage, ScheduleShowRequest, SessionRef, ThreadSubscriptionState,
        ThreadSubscriptionsRequest, WakeShowRequest, WakeState,
    },
};
use std::{fmt, path::Path};

#[path = "owned_resource_cleanup_producer_report.rs"]
mod producer_report;
use producer_report::{ActualHelperPrefix, check_actual_helper_report, missing_installed_pairs};

#[path = "owned_resource_cleanup_runtime_fixture.rs"]
mod runtime_fixture;
use runtime_fixture::{
    ActorRootExpectation, ExpectedResources, FixtureCleanupIssue, RuntimeCleanupFixture,
    RuntimePrefix, ensure,
};

#[path = "owned_resource_cleanup_state_checks.rs"]
mod state_checks;
use state_checks::{
    StoreExpectation, check_fixture_stores, check_public_absent_roots, check_public_state,
    check_reopened_absent_roots, check_reopened_stores,
};
#[path = "owned_resource_cleanup_producer_runtime_tests.rs"]
mod producer_runtime_tests;

#[tokio::test]
async fn runtime_error_after_wake_receipt_retires_known_prefix() -> ProofResult<()> {
    run_case(RuntimePrefix::WakeReceipt, [1, 0, 4, 4]).await
}
#[tokio::test]
async fn runtime_schedule_before_prepare_is_cleanup_owned() -> ProofResult<()> {
    run_case(RuntimePrefix::BeforePrepare, [1, 1, 4, 4]).await
}
#[tokio::test]
async fn runtime_enabled_peer_schedule_becomes_disabled() -> ProofResult<()> {
    run_case(RuntimePrefix::EnabledSchedule, [1, 1, 4, 4]).await
}
#[tokio::test]
async fn runtime_first_reader_is_retired_before_second_root_exists() -> ProofResult<()> {
    run_case(RuntimePrefix::FirstReader, [0, 0, 2, 2]).await
}
#[tokio::test]
async fn stopped_runtime_keeps_all_cleanup_attempts_unknown_without_retry() -> ProofResult<()> {
    run_case(RuntimePrefix::UnavailableApi, [1, 1, 4, 4]).await
}

async fn run_case(prefix: RuntimePrefix, expected_counts: [usize; 4]) -> ProofResult<()> {
    let directory = tempfile::Builder::new()
        .prefix("u6-l3-")
        .tempdir_in("/tmp")?;
    let mut fixture = RuntimeCleanupFixture::allocate(directory.path())?;
    let scenario: ProofResult<()> =
        async {
            fixture.start().await?;
            fixture.build_prefix(prefix).await?;
            check_public_state(
                &fixture
                    .owned
                    .as_ref()
                    .ok_or("fixture client missing")?
                    .client,
                &fixture.expected,
                StoreExpectation::Unchanged(prefix),
            )
            .await?;
            match prefix {
                RuntimePrefix::FirstReader
                | RuntimePrefix::WakeReceipt
                | RuntimePrefix::BeforePrepare => Err(Box::new(InjectedPrefixFailure(prefix))
                    as Box<dyn std::error::Error + Send + Sync>),
                RuntimePrefix::EnabledSchedule | RuntimePrefix::UnavailableApi => Ok(()),
            }
        }
        .await;
    // Preserve the scenario result until every cleanup, teardown and reopen operation finishes.
    let stopped_api = if prefix == RuntimePrefix::UnavailableApi {
        fixture.stop_runtime().await
    } else {
        Ok(())
    };
    let expected = fixture.expected.clone();
    let store_expectation = if prefix == RuntimePrefix::UnavailableApi {
        StoreExpectation::Unchanged(prefix)
    } else {
        StoreExpectation::Retired
    };
    let cleanup_observation: ProofResult<()> = async {
        let owned = fixture
            .owned
            .take()
            .ok_or("no actual client/owner available for cleanup")?;
        let report =
            cleanup_owned_resources(&owned.client, owned.resources.into_cleanup_plan()).await;
        check_report(
            &report,
            &expected,
            expected_counts,
            prefix == RuntimePrefix::UnavailableApi,
        )?;
        if prefix != RuntimePrefix::UnavailableApi {
            check_public_state(&owned.client, &expected, StoreExpectation::Retired).await?;
        }
        check_fixture_stores(&mut fixture, store_expectation).await
    }
    .await;
    let teardown = fixture.finish().await;
    let reopen = check_reopened_stores(directory.path(), &expected, store_expectation).await;
    let cleanup_succeeded =
        stopped_api.is_ok() && cleanup_observation.is_ok() && teardown.is_empty() && reopen.is_ok();
    let scenario = assess_scenario(prefix, scenario, cleanup_succeeded);
    match (scenario, stopped_api, cleanup_observation, teardown, reopen) {
        (Ok(()), Ok(()), Ok(()), teardown, Ok(())) if teardown.is_empty() => Ok(()),
        (scenario, stopped_api, cleanup_observation, teardown, reopen) => {
            Err(Box::new(RuntimeCaseFailure {
                scenario: scenario.err(),
                stopped_api: stopped_api.err(),
                cleanup_observation: cleanup_observation.err(),
                teardown,
                reopen: reopen.err(),
            }))
        }
    }
}

fn check_report(
    report: &MatrixCleanupReport,
    expected: &ExpectedResources,
    expected_counts: [usize; 4],
    unavailable: bool,
) -> ProofResult<()> {
    let mut counts = [0_usize; 4];
    let [wake_count, schedule_count, unsubscribe_count, unwatch_count] = &mut counts;
    let mut cleanup_operations = Vec::new();
    ensure(
        report.unresolved.is_empty(),
        "known Runtime receipts became unresolved creations",
    )?;
    ensure(
        report.history.actor_roots.len() == expected.pairs.len(),
        "cleanup lost actor/root history",
    )?;
    for pair in &expected.pairs {
        ensure(
            report.history.actor_roots.iter().any(|actual| {
                actual.actor == pair.actor && actual.root_message_id == pair.root_message_id
            }),
            "cleanup changed actual actor/root history",
        )?;
    }
    if let Some(instruction_id) = &expected.instruction {
        ensure(
            matches!(&report.history.instruction, Some(InstructionCreation::Known { instruction_id: actual, .. }) if actual == instruction_id),
            "cleanup discarded actual instruction history",
        )?;
    }
    for attempt in &report.attempts {
        match &attempt.action {
            CleanupAction::CancelWake {
                operation_id,
                wakeup_id,
            } => {
                *wake_count += 1;
                let (creation_operation, actual_id) =
                    expected.wake.as_ref().ok_or("unexpected wake cleanup")?;
                ensure(
                    wakeup_id == actual_id && operation_id != creation_operation,
                    "wake cleanup changed ownership or reused create operation",
                )?;
                cleanup_operations.push(operation_id);
                if !unavailable {
                    ensure(
                        matches!(&attempt.outcome, CleanupOutcome::Returned(CleanupObservation::Wake {
                        state: WakeState::Cancelled, next_due_at: None, pending_delivery_id: None,
                        discarded_delivery_ids, retained_deliveries,
                    }) if discarded_delivery_ids.is_empty() && retained_deliveries.is_empty()),
                        "real wake was not locally Cancelled",
                    )?;
                }
            }
            CleanupAction::DisableSchedule {
                operation_id,
                schedule_id,
            } => {
                *schedule_count += 1;
                let (creation_operation, actual_id) = expected
                    .schedule
                    .as_ref()
                    .ok_or("unexpected schedule cleanup")?;
                ensure(
                    schedule_id == actual_id && operation_id != creation_operation,
                    "schedule cleanup changed ownership or reused create operation",
                )?;
                cleanup_operations.push(operation_id);
                if !unavailable {
                    ensure(
                        matches!(
                            &attempt.outcome,
                            CleanupOutcome::Returned(CleanupObservation::Schedule {
                                enabled: false,
                                next_due_at: None,
                                active_run_id: None,
                                waiting_run_id: None,
                                ..
                            })
                        ),
                        "real schedule was not locally disabled",
                    )?;
                }
            }
            CleanupAction::Unsubscribe {
                actor,
                root_message_id,
            } => {
                *unsubscribe_count += 1;
                ensure(
                    expected.pairs.iter().any(|pair| {
                        pair.actor == *actor && pair.root_message_id == *root_message_id
                    }),
                    "unsubscribe targeted an unowned actor/root",
                )?;
                if !unavailable {
                    ensure(
                        matches!(
                            &attempt.outcome,
                            CleanupOutcome::Returned(CleanupObservation::Subscription {
                                delivery_eligible: false,
                            })
                        ),
                        "explicit unsubscribe remained delivery-eligible",
                    )?;
                }
            }
            CleanupAction::Unwatch {
                actor,
                root_message_id,
            } => {
                *unwatch_count += 1;
                ensure(
                    expected.pairs.iter().any(|pair| {
                        pair.actor == *actor && pair.root_message_id == *root_message_id
                    }),
                    "unwatch targeted an unowned actor/root",
                )?;
                if !unavailable {
                    ensure(
                        matches!(
                            &attempt.outcome,
                            CleanupOutcome::Returned(CleanupObservation::Watch { watching: false })
                        ),
                        "explicit unwatch did not report inactive Watch",
                    )?;
                }
            }
        }
        if unavailable {
            ensure(
                matches!(
                    &attempt.outcome,
                    CleanupOutcome::Unknown(_) | CleanupOutcome::Rejected(_)
                ),
                "unavailable Runtime cleanup fabricated a Returned/retired observation",
            )?;
        }
    }
    ensure(
        counts == expected_counts,
        "literal one-attempt cleanup counts changed",
    )?;
    ensure(
        report.attempts.len() == expected_counts.into_iter().sum::<usize>(),
        "cleanup attempt record was lost",
    )?;
    for operation_id in &cleanup_operations {
        ensure(
            cleanup_operations
                .iter()
                .filter(|actual| *actual == operation_id)
                .count()
                == 1,
            "cleanup reused an operation identity",
        )?;
    }
    for pair in &expected.pairs {
        let unsubscribes = report.attempts.iter().filter(|attempt| matches!(&attempt.action,
            CleanupAction::Unsubscribe { actor, root_message_id } if actor == &pair.actor && root_message_id == &pair.root_message_id)).count();
        let unwatches = report.attempts.iter().filter(|attempt| matches!(&attempt.action,
            CleanupAction::Unwatch { actor, root_message_id } if actor == &pair.actor && root_message_id == &pair.root_message_id)).count();
        ensure(
            unsubscribes == 1 && unwatches == 1,
            "actor/root action was replayed or omitted",
        )?;
    }
    Ok(())
}

#[derive(Debug)]
struct InjectedPrefixFailure(RuntimePrefix);
impl fmt::Display for InjectedPrefixFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "injected L3 prefix failure: {:?}", self.0)
    }
}
impl std::error::Error for InjectedPrefixFailure {}
fn assess_scenario(
    prefix: RuntimePrefix,
    result: ProofResult<()>,
    cleanup_succeeded: bool,
) -> ProofResult<()> {
    let expected_error = matches!(
        prefix,
        RuntimePrefix::FirstReader | RuntimePrefix::WakeReceipt | RuntimePrefix::BeforePrepare
    );
    match result {
        Err(error)
            if cleanup_succeeded
                && expected_error
                && error
                    .downcast_ref::<InjectedPrefixFailure>()
                    .is_some_and(|error| error.0 == prefix) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
        Ok(()) if expected_error => Err("expected typed prefix failure was erased".into()),
        Ok(()) => Ok(()),
    }
}
type CaseError = Box<dyn std::error::Error + Send + Sync>;
#[derive(Debug)]
struct RuntimeCaseFailure {
    scenario: Option<CaseError>,
    stopped_api: Option<CaseError>,
    cleanup_observation: Option<CaseError>,
    teardown: Vec<FixtureCleanupIssue>,
    reopen: Option<CaseError>,
}
impl fmt::Display for RuntimeCaseFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Runtime cleanup case failed (scenario={}, API stop={}, observation={}, teardown={}, reopen={})",
            self.scenario.is_some(),
            self.stopped_api.is_some(),
            self.cleanup_observation.is_some(),
            self.teardown.len(),
            self.reopen.is_some()
        )?;
        for issue in &self.teardown {
            write!(formatter, "; {:?} failed", issue.stage)?;
        }
        Ok(())
    }
}
impl std::error::Error for RuntimeCaseFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Some(error) = &self.scenario {
            return Some(error.as_ref());
        }
        if let Some(error) = &self.cleanup_observation {
            return Some(error.as_ref());
        }
        self.teardown
            .first()
            .map(|issue| issue.error.as_ref() as &(dyn std::error::Error + 'static))
    }
}
