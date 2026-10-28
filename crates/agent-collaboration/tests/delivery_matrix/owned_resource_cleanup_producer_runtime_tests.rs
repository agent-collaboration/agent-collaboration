//! Actual helper production, ACL and complete teardown through the native Host.
use super::*;

#[tokio::test]
async fn actual_two_root_producer_uses_distinct_boards_and_preserves_participant_acl()
-> ProofResult<()> {
    use collaboration_client::{
        BoardClientError,
        board::{BoardFailureKind, Identity},
        protocol::{EndpointId, SessionId},
    };
    let directory = tempfile::Builder::new()
        .prefix("u6-l3-producer-")
        .tempdir_in("/tmp")?;
    let mut fixture = RuntimeCleanupFixture::allocate(directory.path())?;
    let mut actors = None;
    let mut produced = None;
    let startup: ProofResult<()> = async {
        Box::pin(fixture.start()).await?;
        let reader = fixture.peer_target()?;
        // Fixture identity only; this does not allocate or load a native session.
        let mut sender = reader.clone();
        sender.endpoint.endpoint_id = EndpointId::try_from("codex-local".to_owned())?;
        sender.session_id = SessionId::try_from("fixture-cleanup-sender".to_owned())?;
        actors = Some([
            serde_json::from_value::<Identity>(
                serde_json::json!({"kind":"session","session":sender}),
            )?,
            serde_json::from_value::<Identity>(
                serde_json::json!({"kind":"session","session":reader}),
            )?,
        ]);
        let owned = fixture
            .owned
            .as_mut()
            .ok_or("actual producer client missing")?;
        // Same actual client and authoritative pre-dispatch owner; no second accumulator.
        produced = Some(
            crate::held_subscription_producer::board_two_thread_subscription_push(
                &owned.client,
                &sender,
                &reader,
                ["L3-FIRST-ROOT-MARKER", "L3-SECOND-ROOT-MARKER"],
                &mut owned.resources,
            )
            .await,
        );
        Ok(())
    }
    .await;
    let prefix = match produced.as_ref() {
        Some(Ok(_)) => ActualHelperPrefix::Completed,
        Some(Err(error))
            if matches!(error.downcast_ref::<BoardClientError>(),
            Some(BoardClientError::Rejected(failure)) if failure.kind == BoardFailureKind::TopLevelPostCooldown) =>
        {
            ActualHelperPrefix::Cooldown
        }
        _ => ActualHelperPrefix::Unqualified,
    };
    // Independent installed-row observations do not adopt, replace or trim cleanup ownership.
    let ownership_inspection = match &actors {
        Some(actors) => fixture.observe_actual_helper_pairs(actors).await,
        None => Err("actual helper actors were not established".into()),
    };
    let producer: ProofResult<()> = async {
        startup?;
        let roots = match produced.take().ok_or("actual helper was not invoked")? {
            Ok(roots) => roots,
            Err(error) => {
                let typed_cooldown = matches!(error.downcast_ref::<BoardClientError>(),
                    Some(BoardClientError::Rejected(failure)) if failure.kind == BoardFailureKind::TopLevelPostCooldown);
                return Err(Box::new(ActualProducerFailure { typed_cooldown, cause: error }) as CaseError);
            }
        };
        let [first_root, second_root] = roots.as_slice() else { return Err("actual helper did not return exactly two roots".into()); };
        ensure(first_root != second_root && fixture.expected.pairs.len() == 4,
            "actual helper did not establish four distinct sender/reader root pairs")?;
        let first = fixture.read_live_thread(first_root).await?;
        let second = fixture.read_live_thread(second_root).await?;
        ensure(first.thread_id == *first_root && second.thread_id == *second_root
            && first.envelope.scope.board != second.envelope.scope.board,
            "actual helper did not create roots on distinct boards")?;
        for actor in actors.as_ref().ok_or("actual helper actors missing")? {
            for root in roots.iter() {
                ensure(fixture.expected.pairs.iter().any(|pair| pair.actor == *actor && pair.root_message_id == *root),
                    "actual helper lost a sender/reader root subscription")?;
            }
        }
        let mut outsider = fixture.peer_target()?;
        outsider.session_id = SessionId::try_from("fixture-cleanup-outsider".to_owned())?;
        let outsider_actor: message_board::Identity = serde_json::from_value(serde_json::json!({"kind":"session","session":outsider}))?;
        let denied = fixture.owned.as_ref().ok_or("actual producer client missing")?.client.thread_post(
            serde_json::from_value::<message_board::ThreadPostRequest>(serde_json::json!({
                "requestId":message_board::RequestId::generate(),"protocolVersion":1,
                "postId":message_board::PostId::generate(),
                "placement":{"kind":"thread","thread":{"thread":first_root}},
                "actor":outsider_actor,"actingFor":null,"addressees":[],
                "text":"Rejected unjoined fixture post","references":[],
            }))?
        ).await?;
        ensure(matches!(denied.result, collaboration_client::board::RequestResult::Decided(
            collaboration_client::board::RequestDecision::Rejected { reasons }
        ) if reasons.iter().any(|reason| matches!(reason, message_board::Refusal::Thread(reason)
            if matches!(reason.as_ref(), message_board::ThreadRefusal::ParticipantRequired { .. })))),
            "unjoined fixture session bypassed thread participant ACL")
    }.await;
    let expected = fixture.expected.clone();
    let mut missing_pairs = Vec::new();
    let cleanup: ProofResult<()> = async {
        let owned = fixture
            .owned
            .take()
            .ok_or("actual producer cleanup owner missing")?;
        let plan = owned.resources.into_cleanup_plan();
        missing_pairs = missing_installed_pairs(&plan.history, &expected);
        let before_absence = check_public_absent_roots(&owned.client, &missing_pairs).await;
        // Even failed absence/readback assertions never prevent any owned cleanup attempt.
        let report = cleanup_owned_resources(&owned.client, plan).await;
        let report_validation =
            check_actual_helper_report(&report, &expected, actors.as_ref(), prefix);
        let public_state =
            check_public_state(&owned.client, &expected, StoreExpectation::Retired).await;
        let after_absence = check_public_absent_roots(&owned.client, &missing_pairs).await;
        let store_state = check_fixture_stores(&mut fixture, StoreExpectation::Retired).await;
        let not_returned = report
            .attempts
            .iter()
            .any(|attempt| !matches!(&attempt.outcome, CleanupOutcome::Returned(_)));
        if before_absence.is_ok()
            && report_validation.is_ok()
            && public_state.is_ok()
            && store_state.is_ok()
            && after_absence.is_ok()
            && !not_returned
            && report.unresolved.is_empty()
        {
            Ok(())
        } else {
            Err(Box::new(ActualProducerCleanupFailure {
                report,
                before_absence: before_absence.err(),
                report_validation: report_validation.err(),
                public_state: public_state.err(),
                store_state: store_state.err(),
                after_absence: after_absence.err(),
            }) as CaseError)
        }
    }
    .await;
    let teardown = fixture.finish().await;
    let reopen =
        check_reopened_stores(directory.path(), &expected, StoreExpectation::Retired).await;
    let absent_reopen = check_reopened_absent_roots(directory.path(), &missing_pairs).await;
    match (
        producer,
        ownership_inspection,
        cleanup,
        teardown,
        reopen,
        absent_reopen,
    ) {
        (Ok(()), Ok(()), Ok(()), teardown, Ok(()), Ok(())) if teardown.is_empty() => Ok(()),
        (producer, ownership_inspection, cleanup, teardown, reopen, absent_reopen) => {
            Err(Box::new(ActualProducerCaseFailure {
                producer: producer.err(),
                ownership_inspection: ownership_inspection.err(),
                cleanup: cleanup.err(),
                teardown,
                reopen: reopen.err(),
                absent_reopen: absent_reopen.err(),
            }) as CaseError)
        }
    }
}

#[derive(Debug)]
struct ActualProducerFailure {
    typed_cooldown: bool,
    cause: CaseError,
}
impl fmt::Display for ActualProducerFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.typed_cooldown {
            formatter.write_str("actual two-root helper rejected TopLevelMessageCooldown")
        } else {
            formatter.write_str("actual two-root helper failed before returning both roots")
        }
    }
}
impl std::error::Error for ActualProducerFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}
#[derive(Debug)]
struct ActualProducerCaseFailure {
    producer: Option<CaseError>,
    ownership_inspection: Option<CaseError>,
    cleanup: Option<CaseError>,
    teardown: Vec<FixtureCleanupIssue>,
    reopen: Option<CaseError>,
    absent_reopen: Option<CaseError>,
}
impl fmt::Display for ActualProducerCaseFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "actual producer case failed (producer={}, ownership={}, cleanup={}, teardown={}, reopen={}, absent reopen={})",
            self.producer.is_some(),
            self.ownership_inspection.is_some(),
            self.cleanup.is_some(),
            self.teardown.len(),
            self.reopen.is_some(),
            self.absent_reopen.is_some()
        )
    }
}
impl std::error::Error for ActualProducerCaseFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.producer
            .as_ref()
            .map(|error| error.as_ref() as &(dyn std::error::Error + 'static))
    }
}

#[derive(Debug)]
struct ActualProducerCleanupFailure {
    report: MatrixCleanupReport,
    before_absence: Option<CaseError>,
    report_validation: Option<CaseError>,
    public_state: Option<CaseError>,
    store_state: Option<CaseError>,
    after_absence: Option<CaseError>,
}
impl fmt::Display for ActualProducerCleanupFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "actual helper cleanup report: {:?}; evidence failures (before absence={}, report={}, public={}, store={}, after absence={})",
            self.report,
            self.before_absence.is_some(),
            self.report_validation.is_some(),
            self.public_state.is_some(),
            self.store_state.is_some(),
            self.after_absence.is_some()
        )
    }
}
impl std::error::Error for ActualProducerCleanupFailure {}
