//! Recovery input is a committed reinstatement; the consumer and both stores stay real.
use super::*;

#[tokio::test]
async fn restart_after_reinstatement_commit_does_not_lose_an_already_consumed_identity() {
    let mut fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.native_runtime_without_board().await;
    let owner = human("board-owner");
    for operation in [
        json!({"kind":"roleGrant","subject":{"kind":"project","subject":fixture.project},"role":"lead","holder":owner,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}),
    ] {
        fixture
            .write_as(
                owner.clone(),
                serde_json::from_value(operation).unwrap(),
                fixture.clock.now(),
            )
            .await;
    }
    let stream = WorkstreamId::generate();
    fixture.write_as(owner.clone(), serde_json::from_value(json!({"kind":"workstreamCreate","workstreamId":stream,"name":"recovery","goal":"reinstated intent"})).unwrap(), fixture.clock.now()).await;
    fixture.write_as(owner, serde_json::from_value(json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":stream},"role":"reviewer","holder":fixture.reader,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null})).unwrap(), fixture.clock.now()).await;
    let post = PostId::generate();
    fixture.write_as(human("writer"), serde_json::from_value(json!({"kind":"threadPost","postId":post,"threadId":fixture.root,"text":"reinstated receiver","references":[],"addressees":[{"kind":"role","recipient":{"kind":"reviewer","subject":stream}}]})).unwrap(), fixture.clock.now()).await;
    let store = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = store.snapshot().await.unwrap();
    let mut intent = read
        .records(RecordKind::NotifyIntent)
        .await
        .unwrap()
        .into_iter()
        .find_map(|record| match record {
            Record::NotifyIntent(intent) if intent.cause == post.clone().into() => Some(intent),
            _ => None,
        })
        .unwrap();
    read.finish().await.unwrap();
    // The NI-2 transaction is outside this consumer proof. Persist its two lifecycle
    // outputs through the real typed writer, with the same identity and recipient.
    intent.set_state(IntentState::Superseded {}).unwrap();
    intent.envelope.request = None;
    let mut write = store
        .begin_admission(crate::test_project_runtime::origin().unwrap())
        .await
        .unwrap();
    write
        .append_record(Record::NotifyIntent(intent.clone()))
        .await
        .unwrap();
    write.commit().await.unwrap();
    let read = store.snapshot().await.unwrap();
    let consumed_through = read.high_water();
    read.finish().await.unwrap();
    fixture
        .interactions
        .mark_intent_consumed(
            &fixture.project,
            &intent.intent_id,
            current_intent_cursor(&fixture, &intent.intent_id).await,
        )
        .await
        .unwrap();
    fixture
        .interactions
        .advance_intents_scanned_through(&fixture.project, consumed_through)
        .await
        .unwrap();
    // Negative control: the consumed old state stays skipped while it is current.
    runtime
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "old consumed version produced a push"
    );
    runtime.close().await;

    intent.set_state(IntentState::Emitted {}).unwrap();
    let mut write = store
        .begin_admission(crate::test_project_runtime::origin().unwrap())
        .await
        .unwrap();
    let reinstated_at = write
        .append_record(Record::NotifyIntent(intent.clone()))
        .await
        .unwrap();
    write.commit().await.unwrap();
    assert!(reinstated_at.position > consumed_through.position);
    // Crash cut: re-resolution committed, before any in-memory replacements list is
    // processed. Reopen actual native message state; no saved reply survives this cut.
    fixture = fixture.reopen_messages().await;
    let restarted = fixture.native_runtime_without_board().await;
    restarted
        .completions
        .send(collaboration_protocol::DeliveryOutcome::Started)
        .unwrap();
    restarted
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    let request = restarted
        .requests
        .lock()
        .await
        .try_recv()
        .expect("committed reinstatement was suppressed by the old consumed ID");
    let push = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(push.header_facts, collaboration_protocol::PushHeaderFacts::BoardNotification { intent: delivered, .. } if delivered == intent.intent_id)
    );
    restarted
        .publisher
        .consume_project_intents(&fixture.project)
        .await
        .unwrap();
    assert!(
        restarted.requests.lock().await.try_recv().is_err(),
        "recovered intent produced a duplicate push"
    );
    assert!(
        fixture
            .interactions
            .intent_consumed(&fixture.project, &intent.intent_id, consumed_through)
            .await
            .unwrap(),
        "old consumed version was erased"
    );
    restarted.close().await;
}
