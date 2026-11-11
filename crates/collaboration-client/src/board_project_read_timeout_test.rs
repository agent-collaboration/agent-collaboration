use crate::{CollaborationClient, api_connection::DEFAULT_CALL_TIMEOUT, scripted_api::ScriptedApi};
use message_board::ProjectRecordListRequest;
use router_clock::{ClockInstant, ManualClock};
use serde_json::json;
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn project_read_survives_the_old_thirty_second_cutoff() {
    let clock = Arc::new(ManualClock::new("2026-11-11T00:00:00Z".parse().unwrap()));
    let (api, mut calls) = ScriptedApi::start_with_clock(clock.clone()).unwrap();
    let client = CollaborationClient::connect_with_clock(
        api.directory(),
        "project-read-proof",
        "1",
        clock.clone(),
    )
    .await
    .unwrap();
    let request: ProjectRecordListRequest = serde_json::from_value(json!({
        "projectId":"01a1635f-5d9d-70e4-bfc7-6d96c1a210e7",
        "page":{"limit":100,"cursor":null}
    }))
    .unwrap();
    let mut reading = tokio::spawn(async move { client.workstream_list(request).await });
    let call = calls.next().await.unwrap();
    assert_eq!(call.tool, "workstream_list");
    // Hold the real transport reply until either the premature cutoff fires or
    // the longer deadline has been checked at the newly advanced manual time.
    clock.advance(Duration::from_secs(40)).unwrap();
    tokio::select! {
        result = &mut reading => panic!("project read ended at the old cutoff: {result:?}"),
        () = clock.not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(185))) => {}
    }
    call.succeed(json!({"records":[],"nextCursor":null}));
    let result = reading.await.unwrap().unwrap();
    assert!(result.records.is_empty());
    assert!(result.next_cursor.is_none());
}

#[tokio::test]
async fn unanswered_project_read_has_a_finite_transport_deadline() {
    let clock = Arc::new(ManualClock::new("2026-11-11T00:00:00Z".parse().unwrap()));
    let (api, mut calls) = ScriptedApi::start_with_clock(clock.clone()).unwrap();
    let client = CollaborationClient::connect_with_clock(
        api.directory(),
        "project-read-proof",
        "1",
        clock.clone(),
    )
    .await
    .unwrap();
    let request: ProjectRecordListRequest = serde_json::from_value(json!({
        "projectId":"01a1635f-5d9d-70e4-bfc7-6d96c1a210e7",
        "page":{"limit":100,"cursor":null}
    }))
    .unwrap();
    let mut reading = tokio::spawn(async move { client.workstream_list(request).await });
    let unanswered = calls.next().await.unwrap();
    clock.advance(Duration::from_secs(184)).unwrap();
    tokio::select! {
        result = &mut reading => panic!("project read ended before its finite deadline: {result:?}"),
        () = clock.not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(185))) => {}
    }
    clock.advance(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        reading.await.unwrap(),
        Err(crate::BoardClientError::Connection(
            crate::ClientError::Timeout
        ))
    ));
    drop(unanswered);
    assert_eq!(DEFAULT_CALL_TIMEOUT, Duration::from_secs(30));
}

#[tokio::test]
async fn ordinary_api_and_mutation_calls_keep_their_thirty_second_bound() {
    for mutation in [false, true] {
        let clock = Arc::new(ManualClock::new("2026-11-11T00:00:00Z".parse().unwrap()));
        let (api, mut calls) = ScriptedApi::start_with_clock(clock.clone()).unwrap();
        let client = CollaborationClient::connect_with_clock(
            api.directory(),
            "default-timeout-proof",
            "1",
            clock.clone(),
        )
        .await
        .unwrap();
        let workspace_id = message_board::WorkspaceId::generate();
        let expected = workspace_id.clone();
        let mut operation = tokio::spawn(async move {
            if mutation {
                client
                    .workspace_create(message_board::WorkspaceCreateRequest {
                        request_id: message_board::RequestId::generate(),
                        protocol_version: message_board::ProtocolVersion::CURRENT,
                        workspace_id,
                        name: "Timeout fixture".to_owned().try_into().unwrap(),
                        description: String::new().try_into().unwrap(),
                        actor: message_board::Identity::Human {
                            human_id: "fixture".to_owned().try_into().unwrap(),
                        },
                        acting_for: None,
                    })
                    .await
                    .map(|_| ())
            } else {
                client
                    .connection
                    .call("fixture_ordinary_read", json!({}))
                    .await
                    .map(|_| ())
                    .map_err(crate::BoardClientError::Connection)
            }
        });
        let unanswered = calls.next().await.unwrap();
        clock.advance(Duration::from_secs(29)).unwrap();
        tokio::select! {
            _ = &mut operation => panic!("default call ended before its deadline"),
            () = clock.not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(30))) => {}
        }
        clock.advance(Duration::from_secs(1)).unwrap();
        let result = operation.await.unwrap();
        if mutation {
            assert!(
                matches!(result, Err(crate::BoardClientError::OutcomeUnknown {
                resource: message_board::ResourceIdentity::Workspace { workspace_id }, ..
            }) if workspace_id == expected)
            );
        } else {
            assert!(matches!(
                result,
                Err(crate::BoardClientError::Connection(
                    crate::ClientError::Timeout
                ))
            ));
        }
        drop(unanswered);
    }
}
