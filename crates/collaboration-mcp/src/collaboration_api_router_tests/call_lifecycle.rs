//! Calls release their real native waits on disconnect and deadline.
use super::*;

#[tokio::test]
async fn a_client_that_disconnects_cancels_its_call_on_both_listeners() {
    // Arrange
    let directory = private_directory();
    let config = api_config(wake_application(directory.path()).await, directory.path());
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let wake = tcp.call_tool("wake_send", wake_send_arguments()).await;
    let wakeup_id = structured(&wake)["definition"]["wakeupId"].clone();
    let long_wait = json!({"wakeupId": wakeup_id, "timeoutSeconds": 1500});

    for api in [&tcp, &unix] {
        // Act: the call is in flight, then its client goes away.
        api.abandon_tool_call("wake_wait_until_first_fire", long_wait.clone())
            .await;

        // Assert: the handler was cancelled and released the Router, long before its bound.
        api.await_active_calls(0, Duration::from_secs(5)).await;
    }
    tcp.stop().await;
    unix.stop().await;
}

#[tokio::test]
async fn a_client_that_disconnects_from_a_mutation_waiting_on_the_store_leaves_no_mutation() {
    // A durable creation survives caller cancellation. This plain mutation still
    // belongs to the caller's future, so cancelling it must release its native wait.
    let directory = private_directory();
    let (admission, registry, fleet, fleet_runtime) = project_admission(directory.path()).await;
    let config = api_config(
        (test_identity().with_project_admission(admission.clone())).build(),
        directory.path(),
    );
    let tcp = ServedApi::tcp(&config).await;
    let unix = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let board = board_scope(&tcp).await;
    let project = ProjectId::generate();
    let create = project_create_arguments("Cancellation setup", &board, project.clone());
    crate::native_board_test_fixture::prepare_registered_creation(
        &fleet,
        &registry,
        &serde_json::from_value(create.clone()).unwrap(),
        &message_board::ActorOrigin {
            installation: TEST_SERVICE_ID.to_owned().try_into().unwrap(),
            machine: "API proof".to_owned().try_into().unwrap(),
        },
    )
    .await
    .expect("accepted registered creation input");
    let created = tcp.call_tool("project_create", create).await;
    assert_eq!(
        structured(&created)["result"]["decision"]["kind"],
        "accepted"
    );
    let grant = tcp.call_tool("role_grant", json!({
        "requestId":RequestId::generate(),"protocolVersion":1,"actor":owner(),"actingFor":null,
        "projectId":project,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":owner(),"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}
    })).await;
    assert_eq!(
        structured(&grant)["result"]["decision"]["kind"],
        "accepted",
        "{grant:?}"
    );
    let store = registry.existing_project(&project).await.unwrap().unwrap();
    let mut read = store.snapshot().await.unwrap();
    let before_head = read.high_water();
    let before_requests = read
        .records(message_board::RecordKind::Request)
        .await
        .unwrap();
    assert!(
        read.records(message_board::RecordKind::Workstream)
            .await
            .unwrap()
            .is_empty()
    );
    read.finish().await.unwrap();
    let database = registry
        .database(&project)
        .await
        .expect("native project database");
    let held = database.begin_write().await.expect("native writer guard");
    let mut abandoned = Vec::new();
    for (api, name) in [(&tcp, "Abandoned over TCP"), (&unix, "Abandoned over Unix")] {
        let request = RequestId::generate();
        let input = json!({
            "requestId":request,"protocolVersion":1,"actor":owner(),"actingFor":null,
            "projectId":project,"operation":{"kind":"workstreamCreate","workstreamId":message_board::WorkstreamId::generate(),"name":name,"goal":""}
        });
        api.abandon_tool_call("workstream_create", input.clone())
            .await;
        abandoned.push((request, input, api));
        api.await_active_calls(0, Duration::from_secs(5)).await;
    }
    drop(held);
    let mut read = store.snapshot().await.unwrap();
    assert!(
        read.records(message_board::RecordKind::Workstream)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        read.records(message_board::RecordKind::Request)
            .await
            .unwrap(),
        before_requests
    );
    assert_eq!(read.high_water(), before_head);
    for (request, _, _) in &abandoned {
        assert!(
            read.record(&request.clone().into())
                .await
                .unwrap()
                .is_none()
        );
    }
    read.finish().await.unwrap();
    // A valid retry is a sensitivity control: neither an invalid tool input nor
    // a disabled mutation path may make the no-effect cancellation check pass.
    for (_, input, api) in abandoned {
        let reply = api.call_tool("workstream_create", input).await;
        assert_eq!(
            structured(&reply)["result"]["decision"]["kind"],
            "accepted",
            "{reply:?}"
        );
    }
    tcp.stop().await;
    unix.stop().await;
    fleet_runtime
        .shutdown()
        .await
        .expect("stop real central fixture");
}

#[tokio::test]
async fn a_wake_wait_ends_at_its_bound_while_the_store_is_held() {
    // Arrange: a wake-up, then its store held by someone else past the waits' one-second bound.
    let directory = private_directory();
    let automation = automation_store(directory.path()).await;
    let config = api_config(
        (test_identity().with_automation_store(Arc::clone(&automation))).build(),
        directory.path(),
    );
    let api = ServedApi::unix(&config, &directory.path().join("api.sock"));
    let wake = api.call_tool("wake_send", wake_send_arguments()).await;
    let wakeup_id = structured(&wake)["definition"]["wakeupId"].clone();
    let cursor = structured(&wake)["latestEventCursor"].clone();
    let held = automation.lock().await;

    // Act: one wait starts from the current state and one resumes from the cursor; both need
    // the held store before they can observe anything.
    tokio::time::pause();
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            api.call_tool(
                "wake_wait_until_first_fire",
                json!({"wakeupId": wakeup_id, "timeoutSeconds": 1}),
            ),
            api.call_tool(
                "wake_wait_until_first_fire",
                json!({"wakeupId": wakeup_id, "after": cursor, "timeoutSeconds": 1}),
            ),
        )
    })
    .await;
    tokio::time::resume();
    drop(held);

    // Assert: both ended at their bound while the store was still held. With no cursor yet
    // the start is unavailable; the resumed wait times out at the cursor it was given.
    let (starting, resuming) = ended.expect("the waits ended at their bound");
    assert_eq!(starting.is_error, Some(true), "{starting:?}");
    assert_eq!(structured(&starting)["kind"], "waitUnavailable");
    assert_eq!(structured(&resuming)["outcome"]["kind"], "timedOut");
    assert_eq!(structured(&resuming)["cursor"], cursor);
    api.stop().await;
}
