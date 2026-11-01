//! Public observations while live; exact native state only after the owner stops.
use super::*;

pub(super) async fn check_public_state(
    client: &CollaborationClient,
    expected: &ExpectedResources,
    expectation: StoreExpectation,
) -> ProofResult<()> {
    let retired = matches!(expectation, StoreExpectation::Retired);
    if let Some((_, wakeup_id)) = &expected.wake {
        let wake = client
            .read_wakeup(WakeShowRequest {
                wakeup_id: wakeup_id.clone(),
            })
            .await?;
        ensure(
            wake.definition.wakeup_id == *wakeup_id
                && wake.state
                    == if retired {
                        WakeState::Cancelled
                    } else {
                        WakeState::Active
                    }
                && wake.pending_delivery_id.is_none()
                && wake.next_due_at.is_none() == retired,
            "public wake read did not confirm the expected state",
        )?;
    }
    if let Some((_, schedule_id)) = &expected.schedule {
        let schedule = client
            .read_schedule(ScheduleShowRequest {
                schedule_id: schedule_id.clone(),
            })
            .await?;
        let enabled = matches!(
            expectation,
            StoreExpectation::Unchanged(RuntimePrefix::EnabledSchedule)
        );
        ensure(
            schedule.schedule_id == *schedule_id
                && schedule.definition.enabled == enabled
                && schedule.next_due_at.is_some() == enabled
                && schedule.active_run_id.is_none()
                && schedule.waiting_run_id.is_none(),
            "public schedule read did not confirm the expected state",
        )?;
    }
    let mut checked_actors = Vec::new();
    for pair in &expected.pairs {
        if !checked_actors.contains(&&pair.actor) {
            let subscriptions = client
                .thread_subscriptions(ThreadSubscriptionsRequest {
                    actor: pair.actor.clone(),
                })
                .await?;
            ensure(
                subscriptions.unavailable.is_empty(),
                "public subscription observation contained an unavailable project",
            )?;
            if retired {
                ensure(
                    subscriptions.subscriptions.is_empty(),
                    "public active/draining subscription list was not empty",
                )?;
            } else {
                for owned in expected
                    .pairs
                    .iter()
                    .filter(|owned| owned.actor == pair.actor)
                {
                    ensure(
                        subscriptions.subscriptions.iter().any(|record| {
                            record.scope
                                == message_board::SubscriptionScope::thread(
                                    owned.root_message_id.clone(),
                                )
                                && record.state == ThreadSubscriptionState::Active
                        }),
                        "public subscription was not Active before cleanup",
                    )?;
                }
            }
            checked_actors.push(&pair.actor);
        }
    }
    for pair in &expected.pairs {
        let thread = client
            .thread_show(ThreadShowRequest {
                thread: pair.root_message_id.clone().into(),
                reader: message_board::ReadActor::Identified {
                    reader: pair.actor.clone(),
                },
                include_redacted: false,
            })
            .await?;
        ensure(
            thread.thread.thread_id == pair.root_message_id
                && thread
                    .watch_status
                    .is_some_and(|watch| watch.watching != retired),
            "public exact-reader Watch did not match the expected state",
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum StoreExpectation {
    Retired,
    Unchanged(RuntimePrefix),
}

pub(super) async fn check_fixture_stores(
    fixture: &mut RuntimeCleanupFixture,
    expectation: StoreExpectation,
) -> ProofResult<()> {
    fixture.open_stopped_stores().await?;
    let board = fixture
        .board
        .as_mut()
        .ok_or("fixture-owned cleanup readback was not opened")?;
    let automation = fixture
        .automation
        .as_mut()
        .ok_or("fixture-owned AutomationStore was not opened")?;
    check_store_state(board, automation, &fixture.expected, expectation).await
}
async fn check_store_state(
    board: &mut CleanupReadback,
    automation: &mut AutomationStore,
    expected: &ExpectedResources,
    expectation: StoreExpectation,
) -> ProofResult<()> {
    let retired = matches!(expectation, StoreExpectation::Retired);
    for pair in &expected.pairs {
        let record = board
            .subscription_state(&pair.actor, &pair.root_message_id)
            .await?
            .ok_or("exact actor/root subscription history disappeared")?;
        let state_matches = matches!(
            (retired, record.state()),
            (
                true,
                SubscriptionState::Ended {
                    reason: EndReason::Cancelled
                }
            ) | (false, SubscriptionState::Active)
        );
        ensure(
            state_matches,
            "exact SQLite subscription was not the literal expected local state",
        )?;
        if retired {
            ensure(
                record
                    .roots()
                    .iter()
                    .map(|root| root.pending_count())
                    .sum::<u64>()
                    == 0,
                "exact stopped subscription retained pending work",
            )?;
        }
        ensure(
            board
                .watch_active(&pair.actor, &pair.root_message_id)
                .await?
                != retired,
            "exact SQLite Watch state changed unexpectedly",
        )?;
    }
    if let Some((_, wakeup_id)) = &expected.wake {
        let wake = automation.read_wakeup::<SavedMessage>(wakeup_id).await?;
        let state = if retired {
            agent_automation::WakeState::Cancelled
        } else {
            agent_automation::WakeState::Active
        };
        ensure(
            wake.definition.wakeup_id == *wakeup_id
                && wake.state == state
                && wake.pending_delivery_id.is_none()
                && wake.next_due_at_ms.is_none() == retired,
            "SQLite wake did not preserve exact local state",
        )?;
    }
    if let Some((_, schedule_id)) = &expected.schedule {
        let schedule = automation
            .inspect_schedule::<SessionRef, EndpointRef, message_board::ProjectId>(schedule_id)
            .await?;
        let enabled = matches!(
            expectation,
            StoreExpectation::Unchanged(RuntimePrefix::EnabledSchedule)
        );
        ensure(
            schedule.record.schedule_id == *schedule_id
                && schedule.record.definition.enabled == enabled
                && schedule.record.next_due_at_ms.is_some() == enabled
                && schedule.active_run_id.is_none()
                && schedule.waiting_run_id.is_none(),
            "SQLite schedule changed or invented run cancellation",
        )?;
    }
    Ok(())
}

pub(super) async fn check_reopened_stores(
    directory: &Path,
    expected: &ExpectedResources,
    expectation: StoreExpectation,
) -> ProofResult<()> {
    let mut board = None;
    let mut automation = None;
    let observed: ProofResult<()> = async {
        board = Some(CleanupReadback::open(directory).await?);
        automation = Some(AutomationStore::open(&directory.join("automation.sqlite")).await?);
        check_store_state(
            board.as_mut().ok_or("reopened cleanup readback missing")?,
            automation
                .as_mut()
                .ok_or("reopened AutomationStore missing")?,
            expected,
            expectation,
        )
        .await
    }
    .await;
    let board_close: ProofResult<()> = match board {
        Some(board) => board.close().await,
        None => Ok(()),
    };
    let automation_close: ProofResult<()> = match automation {
        Some(automation) => automation.close().await.map_err(Into::into),
        None => Ok(()),
    };
    match (observed, board_close, automation_close) {
        (Ok(()), Ok(()), Ok(())) => Ok(()),
        (observed, board_close, automation_close) => Err(Box::new(ReopenFailure {
            observed: observed.err(),
            board_close: board_close.err(),
            automation_close: automation_close.err(),
        })),
    }
}

#[derive(Debug)]
struct ReopenFailure {
    observed: Option<CaseError>,
    board_close: Option<CaseError>,
    automation_close: Option<CaseError>,
}
impl fmt::Display for ReopenFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "fixture SQLite reopen failed (read={}, board close={}, automation close={})",
            self.observed.is_some(),
            self.board_close.is_some(),
            self.automation_close.is_some()
        )
    }
}
impl std::error::Error for ReopenFailure {}

pub(super) async fn check_public_absent_roots(
    client: &CollaborationClient,
    pairs: &[ActorRootExpectation],
) -> ProofResult<()> {
    for pair in pairs {
        let subscriptions = client
            .thread_subscriptions(ThreadSubscriptionsRequest {
                actor: pair.actor.clone(),
            })
            .await?;
        ensure(
            subscriptions.unavailable.is_empty()
                && !subscriptions.subscriptions.iter().any(|record| {
                    record.scope
                        == message_board::SubscriptionScope::thread(pair.root_message_id.clone())
                }),
            "never-created helper subscription appeared or read was unavailable",
        )?;
        let shown = client
            .thread_show(ThreadShowRequest {
                thread: pair.root_message_id.clone().into(),
                reader: message_board::ReadActor::Identified {
                    reader: pair.actor.clone(),
                },
                include_redacted: false,
            })
            .await;
        ensure(
            matches!(shown, Err(collaboration_client::BoardClientError::Rejected(ref failure))
            if failure.kind == message_board::BoardFailureKind::ResourceNotFound),
            "never-created helper root appeared or public absence was inconclusive",
        )?;
    }
    Ok(())
}
async fn check_absent_roots(
    board: &mut CleanupReadback,
    pairs: &[ActorRootExpectation],
) -> ProofResult<()> {
    for pair in pairs {
        ensure(
            board
                .subscription_state(&pair.actor, &pair.root_message_id)
                .await?
                .is_none(),
            "never-created helper subscription appeared",
        )?;
        ensure(
            board.thread_record(&pair.root_message_id).await?.is_none(),
            "never-created helper root appeared or absence read was inconclusive",
        )?;
    }
    Ok(())
}
pub(super) async fn check_reopened_absent_roots(
    directory: &Path,
    pairs: &[ActorRootExpectation],
) -> ProofResult<()> {
    let mut board = CleanupReadback::open(directory).await?;
    let observed = check_absent_roots(&mut board, pairs).await;
    let board_close: ProofResult<()> = board.close().await;
    match (observed, board_close) {
        (Ok(()), Ok(())) => Ok(()),
        (observed, board_close) => Err(Box::new(ReopenFailure {
            observed: observed.err(),
            board_close: board_close.err(),
            automation_close: None,
        })),
    }
}
