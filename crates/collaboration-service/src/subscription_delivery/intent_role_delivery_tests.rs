//! Claims and the actual consumer preserve a handed-off role recipient through replacement.
use super::*;

#[tokio::test]
async fn handed_off_role_intent_delivers_to_the_frozen_recipient_after_holders_change() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let owner = human("board-owner");
    let stream = WorkstreamId::generate();
    let replacement: Identity = serde_json::from_value(json!({"kind":"session","session":{
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"replacement-reviewer"
    }})).unwrap();
    for operation in [
        json!({"kind":"roleGrant","subject":{"kind":"project","subject":fixture.project},"role":"lead","holder":owner,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}),
        json!({"kind":"workstreamCreate","workstreamId":stream,"name":"frozen recipient","goal":"claim then replace"}),
        json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":stream},"role":"reviewer","holder":fixture.reader,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}),
    ] {
        fixture
            .write_as(
                owner.clone(),
                serde_json::from_value(operation).unwrap(),
                fixture.clock.now(),
            )
            .await;
    }
    let post = PostId::generate();
    let reply = fixture.write_as(human("writer"), serde_json::from_value(json!({"kind":"threadPost","postId":post,"threadId":fixture.root,"text":"frozen addressed cause","references":[],"addressees":[{"kind":"role","recipient":{"kind":"reviewer","subject":stream}}]})).unwrap(), fixture.clock.now()).await;
    let RequestResult::Decided(RequestDecision::Accepted { effects, .. }) = reply.result else {
        panic!("cause accepted");
    };
    let key = effects
        .entries()
        .iter()
        .map(|(reference, _)| reference)
        .find(|record| record.kind == RecordKind::NotifyIntent)
        .unwrap();
    let intent = NotifyIntentId::try_from(key.id.as_str().to_owned()).unwrap();
    assert!(
        matches!(fixture.admission.claim_intent(&fixture.project, &intent, intent_handoff_input()).await.unwrap(), crate::IntentReply::Recorded { claim: Some(IntentClaim::Claimed { replacements, .. }), .. } if replacements.is_empty())
    );
    for operation in [
        json!({"kind":"roleRelease","subject":{"kind":"workstream","subject":stream},"role":"reviewer","holder":RecordedActor::declared(fixture.reader.clone()),"expectedRevision":1}),
        json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":stream},"role":"reviewer","holder":replacement,"model":null,"lineage":null,"expectedRevision":2,"ownerGrant":null}),
    ] {
        fixture
            .write_as(
                owner.clone(),
                serde_json::from_value(operation).unwrap(),
                fixture.clock.now(),
            )
            .await;
    }
    let runtime = fixture.native_runtime_without_board().await;
    // Two role-grant notifications and the addressed post all traverse real push ownership.
    for _ in 0..3 {
        runtime
            .completions
            .send(collaboration_protocol::DeliveryOutcome::Started)
            .unwrap();
    }
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    let mut observed_cause = Vec::new();
    let mut requests = runtime.requests.lock().await;
    while let Ok(request) = requests.try_recv() {
        let push = fixture
            .interactions
            .get_push_record(&request.payload.push_id)
            .await
            .unwrap()
            .unwrap();
        let collaboration_protocol::PushHeaderFacts::BoardNotification {
            project,
            intent: delivered,
            cause,
            ..
        } = push.header_facts
        else {
            panic!("board push");
        };
        assert_eq!(project, fixture.project);
        if cause == post.clone().into() {
            assert_eq!(delivered, intent);
            let Identity::Session { session } = &fixture.reader else {
                panic!("local reader");
            };
            assert_eq!(
                serde_json::to_value(&request.target).unwrap(),
                serde_json::to_value(session).unwrap()
            );
            observed_cause.push(delivered);
        }
    }
    assert_eq!(
        observed_cause,
        vec![intent.clone()],
        "exactly the frozen intent reached Layer-0"
    );
    drop(requests);
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store
        .begin_admission(crate::test_project_runtime::origin().unwrap())
        .await
        .unwrap();
    let cause_intents = read.cause_intents(&post.into()).await.unwrap();
    assert_eq!(
        cause_intents.len(),
        1,
        "handoff must not add a new-holder intent"
    );
    assert_eq!(cause_intents.first().unwrap().intent_id, intent);
    assert!(
        matches!(cause_intents.first().unwrap().recipient(), IntentRecipient::Resolved(recipient) if recipient.actor.declared_identity().as_ref() == Some(&fixture.reader))
    );
    read.rollback().await.unwrap();
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "frozen push repeated"
    );
    runtime.close().await;
}

#[tokio::test]
async fn deferred_human_claim_delivers_a_new_local_coholder_in_the_same_consumer_pass() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let owner = human("board-owner");
    let deferred = human("deferred-reviewer");
    let stream = WorkstreamId::generate();
    for operation in [
        json!({"kind":"roleGrant","subject":{"kind":"project","subject":fixture.project},"role":"lead","holder":owner,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}),
        json!({"kind":"workstreamCreate","workstreamId":stream,"name":"live replacements","goal":"retain human and add session"}),
        json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":stream},"role":"reviewer","holder":deferred,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}),
    ] {
        fixture
            .write_as(
                owner.clone(),
                serde_json::from_value(operation).unwrap(),
                fixture.clock.now(),
            )
            .await;
    }
    let post = PostId::generate();
    fixture.write_as(human("writer"), serde_json::from_value(json!({"kind":"threadPost","postId":post,"threadId":fixture.root,"text":"deferred recipient adds local recipient","references":[],"addressees":[{"kind":"role","recipient":{"kind":"reviewer","subject":stream}}]})).unwrap(), fixture.clock.now()).await;
    fixture.write_as(owner, serde_json::from_value(json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":stream},"role":"reviewer","holder":fixture.reader,"model":null,"lineage":null,"expectedRevision":1,"ownerGrant":null})).unwrap(), fixture.clock.now()).await;
    let runtime = fixture.native_runtime_without_board().await;
    for _ in 0..2 {
        runtime
            .completions
            .send(collaboration_protocol::DeliveryOutcome::Started)
            .unwrap();
    }
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    let mut matched = Vec::new();
    let mut requests = runtime.requests.lock().await;
    while let Ok(request) = requests.try_recv() {
        let push = fixture
            .interactions
            .get_push_record(&request.payload.push_id)
            .await
            .unwrap()
            .unwrap();
        if let collaboration_protocol::PushHeaderFacts::BoardNotification { intent, cause, .. } =
            push.header_facts
            && cause == post.clone().into()
        {
            let Identity::Session { session } = &fixture.reader else {
                panic!("local reader");
            };
            assert_eq!(
                serde_json::to_value(request.target).unwrap(),
                serde_json::to_value(session).unwrap()
            );
            matched.push(intent);
        }
    }
    assert_eq!(
        matched.len(),
        1,
        "the replacement must be delivered before a later scan"
    );
    drop(requests);
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut write = store
        .begin_admission(crate::test_project_runtime::origin().unwrap())
        .await
        .unwrap();
    let intents = write.cause_intents(&post.into()).await.unwrap();
    assert_eq!(intents.len(), 2);
    assert!(intents.iter().any(|intent| matches!(intent.recipient(), IntentRecipient::Resolved(recipient) if recipient.actor.declared_identity().as_ref() == Some(&deferred)) && matches!(intent.state(), IntentState::Emitted {})));
    assert!(
        intents
            .iter()
            .any(|intent| matched.contains(&intent.intent_id)
                && matches!(intent.state(), IntentState::HandedOff { .. }))
    );
    write.rollback().await.unwrap();
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "same local coholder received twice"
    );
    runtime.close().await;
}

#[tokio::test]
async fn claimed_retained_holder_delivers_an_already_consumed_reinstatement_in_the_same_pass() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let owner = human("board-owner");
    let returning = fixture.reader.clone();
    let mut retained = returning.clone();
    let Identity::Session { session } = &mut retained else {
        panic!("local session fixture");
    };
    session.session_id = SessionId::try_from("retained-reviewer".to_owned()).unwrap();
    let stream = WorkstreamId::generate();
    for operation in [
        json!({"kind":"roleGrant","subject":{"kind":"project","subject":fixture.project},"role":"lead","holder":owner,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}),
        json!({"kind":"workstreamCreate","workstreamId":stream,"name":"live reinstatement","goal":"claim retained actor and revisit consumed identity"}),
        json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":stream},"role":"reviewer","holder":returning,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}),
    ] {
        fixture
            .write_as(
                owner.clone(),
                serde_json::from_value(operation).unwrap(),
                fixture.clock.now(),
            )
            .await;
    }
    let post = PostId::generate();
    fixture.write_as(human("writer"), serde_json::from_value(json!({"kind":"threadPost","postId":post,"threadId":fixture.root,"text":"returning reviewer keeps the original intent","references":[],"addressees":[{"kind":"role","recipient":{"kind":"reviewer","subject":stream}}]})).unwrap(), fixture.clock.now()).await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store
        .begin_admission(crate::test_project_runtime::origin().unwrap())
        .await
        .unwrap();
    let initial = read.cause_intents(&post.clone().into()).await.unwrap();
    assert_eq!(initial.len(), 1);
    let returning_id = initial.first().unwrap().intent_id.clone();
    read.rollback().await.unwrap();
    for operation in [
        json!({"kind":"roleRelease","subject":{"kind":"workstream","subject":stream},"role":"reviewer","holder":RecordedActor::declared(returning.clone()),"expectedRevision":1}),
        json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":stream},"role":"reviewer","holder":retained,"model":null,"lineage":null,"expectedRevision":2,"ownerGrant":null}),
    ] {
        fixture
            .write_as(
                owner.clone(),
                serde_json::from_value(operation).unwrap(),
                fixture.clock.now(),
            )
            .await;
    }
    let crate::IntentReply::Recorded {
        claim: Some(IntentClaim::Superseded { replacements }),
        ..
    } = fixture
        .admission
        .claim_intent(&fixture.project, &returning_id, intent_handoff_input())
        .await
        .unwrap()
    else {
        panic!("removed reviewer is superseded by the new reviewer");
    };
    assert_eq!(replacements.len(), 1);
    let retained_id = replacements.first().unwrap().clone();
    let old_version = current_intent_cursor(&fixture, &returning_id).await;
    assert!(current_intent_cursor(&fixture, &retained_id).await.position > old_version.position);
    {
        let progress = &fixture.interactions;
        progress
            .mark_intent_consumed(&fixture.project, &returning_id, old_version)
            .await
            .unwrap();
        progress
            .advance_intents_scanned_through(&fixture.project, old_version)
            .await
            .unwrap();
    }
    fixture.write_as(owner, serde_json::from_value(json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":stream},"role":"reviewer","holder":returning,"model":null,"lineage":null,"expectedRevision":3,"ownerGrant":null})).unwrap(), fixture.clock.now()).await;
    let mut read = store
        .begin_admission(crate::test_project_runtime::origin().unwrap())
        .await
        .unwrap();
    let before = read.cause_intents(&post.clone().into()).await.unwrap();
    assert_eq!(before.len(), 2);
    assert!(before.iter().any(|intent| intent.intent_id == returning_id
        && matches!(intent.state(), IntentState::Superseded {})));
    assert!(before.iter().any(|intent| intent.intent_id == retained_id
        && matches!(intent.state(), IntentState::Emitted {})));
    read.rollback().await.unwrap();
    let read = store.snapshot().await.unwrap();
    let pass_head = read.high_water();
    read.finish().await.unwrap();
    let runtime = fixture.native_runtime_without_board().await;
    // The current scan ends at pass_head. Reinstatement occurs in A's real claim
    // after that head, so B can arrive in this pass only through Claimed.replacements.
    for _ in 0..3 {
        runtime
            .completions
            .send(collaboration_protocol::DeliveryOutcome::Started)
            .unwrap();
    }
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    // The initial pass submits two addressed posts and the current role grant.
    // Completing those scripted calls proves all three requests reached the
    // queue before its snapshot; a publisher pass only commits their custody.
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst)
            < 3
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("initial addressed and role-grant requests did not reach the client");
    let mut observed = BTreeSet::new();
    let mut first_pass_requests = Vec::new();
    let mut requests = runtime.requests.lock().await;
    while let Ok(request) = requests.try_recv() {
        let push = fixture
            .interactions
            .get_push_record(&request.payload.push_id)
            .await
            .unwrap()
            .unwrap();
        let collaboration_protocol::PushHeaderFacts::BoardNotification {
            project,
            intent,
            cause,
            ..
        } = push.header_facts
        else {
            panic!("board notification");
        };
        first_pass_requests.push((
            request.payload.push_id.clone(),
            request.attempt.clone(),
            request.target.clone(),
            intent.clone(),
            cause.clone(),
        ));
        if cause == post.clone().into() {
            assert_eq!(project, fixture.project);
            let expected = if intent == retained_id {
                &retained
            } else {
                assert_eq!(intent, returning_id, "no new actor identity");
                &returning
            };
            let Identity::Session { session } = expected else {
                panic!("local recipient");
            };
            assert_eq!(
                serde_json::to_value(request.target).unwrap(),
                serde_json::to_value(session).unwrap()
            );
            assert!(observed.insert(intent), "duplicate addressed push");
        }
    }
    assert_eq!(
        observed,
        BTreeSet::from([retained_id.clone(), returning_id.clone()]),
        "Claimed must traverse the already-consumed original B identity before a later scan"
    );
    drop(requests);
    let mut read = store
        .begin_admission(crate::test_project_runtime::origin().unwrap())
        .await
        .unwrap();
    let after = read.cause_intents(&post.into()).await.unwrap();
    assert_eq!(after.len(), 2);
    assert!(
        after
            .iter()
            .all(|intent| matches!(intent.state(), IntentState::HandedOff { .. }))
    );
    read.rollback().await.unwrap();
    assert!(
        current_intent_cursor(&fixture, &returning_id)
            .await
            .position
            > pass_head.position
    );
    assert!(
        fixture
            .interactions
            .intent_consumed(&fixture.project, &returning_id, old_version)
            .await
            .unwrap(),
        "old consumed version remains recorded"
    );
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    let unexpected = runtime.requests.lock().await.try_recv();
    let unexpected_context = match &unexpected {
        Ok(request) => Some((
            request.payload.push_id.clone(),
            request.attempt.clone(),
            request.target.clone(),
            fixture
                .interactions
                .get_push_record(&request.payload.push_id)
                .await
                .map(|record| record.map(|record| record.header_facts)),
        )),
        Err(_) => None,
    };
    eprintln!(
        "reinstatement request diagnostics: first_pass={first_pass_requests:?}; unexpected={unexpected_context:?}; completed_calls={}",
        runtime
            .completed_native_calls
            .load(std::sync::atomic::Ordering::SeqCst)
    );
    assert!(
        unexpected.is_err(),
        "live reinstatement delivered twice: unexpected={unexpected_context:?}; first_pass={first_pass_requests:?}"
    );
    runtime.close().await;
}

fn intent_handoff_input() -> Option<crate::IntentHandoffInput> {
    Some(crate::IntentHandoffInput {
        item: fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        enrollment: crate::IntentRecipientEnrollment {
            service: crate::subscription_delivery::tests::owner_fixture::SERVICE_ID
                .to_owned()
                .try_into()
                .unwrap(),
            peer: fleet_sync_protocol::PeerId::new(
                uuid::Uuid::parse_str("0192a1b2-c3d4-7000-8000-000000000001").unwrap(),
            )
            .unwrap(),
        },
    })
}
