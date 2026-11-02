//! Public observations while live; exact native state only after the owner stops.
use super::*;
#[path = "owned_resource_cleanup_central_readback.rs"]
mod central_readback;

pub(super) async fn check_public_state(
    client: &CollaborationClient,
    expected: &ExpectedResources,
    expectation: StoreExpectation,
) -> ProofResult<()> {
    let retired = matches!(expectation, StoreExpectation::Retired);
    if let Some((_, wakeup_id)) = &expected.wake {
        let collaboration_client::WakeReadResponse::Central { value: wake } = client
            .read_wakeup(WakeShowRequest {
                wakeup_id: wakeup_id.clone(),
            })
            .await?
        else {
            return Err("Runtime Wake read changed to Local/faulted".into());
        };
        ensure(
            wake.definition.wakeup_id == *wakeup_id
                && wake.state
                    == if retired {
                        WakeState::Cancelled
                    } else {
                        WakeState::Active
                    }
                && wake.first_firing.is_none()
                && wake.next_due_at.is_none() == retired,
            "public wake read did not confirm the expected state",
        )?;
        let deliveries = client
            .list_deliveries(collaboration_protocol::DeliveryListRequest {
                wakeup_id: Some(wakeup_id.clone()),
                cursor: None,
                limit: 100.try_into()?,
            })
            .await?;
        ensure(
            deliveries.records.is_empty() && deliveries.next_cursor.is_none(),
            "Runtime Wake fixture invented local delivery obligations",
        )?;
    }
    if let Some((_, schedule_id)) = &expected.schedule {
        let collaboration_client::ScheduleReadResponse::Schedule { value: schedule } = client
            .read_schedule(ScheduleShowRequest {
                schedule_id: schedule_id.clone(),
            })
            .await?
        else {
            return Err("Runtime Schedule read changed to Local/faulted".into());
        };
        let enabled = matches!(
            expectation,
            StoreExpectation::Unchanged(RuntimePrefix::EnabledSchedule)
        );
        ensure(
            schedule.schedule_id == *schedule_id
                && schedule.definition.enabled == enabled
                && schedule.next_due_at.is_some() == enabled,
            "public schedule read did not confirm the expected state",
        )?;
        let firings = client
            .list_firings(collaboration_protocol::FiringListRequest {
                automation_id: schedule_id.as_str().to_owned().try_into()?,
                cursor: None,
                limit: 100.try_into()?,
            })
            .await?;
        ensure(
            firings.records.is_empty() && firings.next_cursor.is_none(),
            "Runtime Schedule fixture admitted central work before cleanup",
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
    central_readback::check_reopened_catalog(&fixture.directory, &fixture.expected, expectation)
        .await?;
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
    if let Some((_, id)) = &expected.wake {
        ensure(
            matches!(
                automation.read_wakeup::<SavedMessage>(id).await,
                Err(automation_storage::StorageError::WakeNotFound)
            ),
            "central cleanup Wake acquired a local shadow definition",
        )?;
        let page = automation
            .list_collection_keys_with_central_parent(
                &automation_storage::AutomationCollection::Deliveries(Some(id.clone())),
                None,
                100,
            )
            .await?;
        ensure(
            page.records.is_empty() && !page.has_more,
            "central cleanup Wake retained local native obligations",
        )?;
    }
    if let Some((_, id)) = &expected.schedule {
        ensure(
            matches!(
                automation
                    .inspect_schedule::<SessionRef, EndpointRef>(id)
                    .await,
                Err(automation_storage::StorageError::ScheduleNotFound)
            ),
            "central cleanup Schedule acquired a local shadow definition",
        )?;
        let page = automation
            .list_collection_keys_with_central_parent(
                &automation_storage::AutomationCollection::Runs(id.clone()),
                None,
                100,
            )
            .await?;
        ensure(
            page.records.is_empty() && !page.has_more,
            "central cleanup Schedule retained local Runs",
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
        central_readback::check_reopened_catalog(directory, expected, expectation).await?;
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
