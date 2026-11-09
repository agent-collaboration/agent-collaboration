//! A machine that stops reading is dropped without holding up any other machine,
//! and a healthy reader's busiest event never overflows its send queue.
use super::{composed_fleet_fixture::*, fleet_mail_dispatch::*};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_central_tokio::SocketCommand;
use fleet_protocol::{PushResult, ServerCall, ServerFrame, ServerRequest};
use runtime_primitives::Clock;
use serde_json::{Value, json};

#[expect(
    clippy::panic_in_result_fn,
    reason = "The stalled machine's drop, the other machine's custody and the resend after reconnect are one journey."
)]
#[tokio::test]
async fn a_full_send_queue_drops_its_reader_at_once_and_the_push_is_resent_after_it_reconnects()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut blocked = Socket::open(&fixture, B, SERVICE_B).await?;
    blocked.fill_send_queue(&fixture).await?;
    let mut responsive = Socket::open(&fixture, C, SERVICE_C).await?;
    let first = envelope(B, SERVICE_B)?;
    let second = envelope(C, SERVICE_C)?;
    // B's push finds its queue full: the submit's event drops B's connection
    // instead of waiting for room.
    tokio::time::timeout(CAP, submit(&fixture, &first)).await??;
    let mut stalled = 0;
    let reason = loop {
        match blocked.next_command().await? {
            SocketCommand::Frame(_) => stalled += 1,
            SocketCommand::Close(reason) => break reason,
        }
    };
    assert!(stalled > 0, "the queue was not full before the push");
    assert_eq!(reason, fleet_protocol::CloseReason::ClientClosed);
    assert_eq!(custody(&fixture, first.id()).await?, ("sent".into(), true));

    // Another machine's push is not held up by B.
    submit(&fixture, &second).await?;
    let frame = responsive.push(second.id()).await?;
    responsive
        .answer(
            &fixture,
            frame,
            PushResult::Accepted {
                accepted_at: fixture.io.now(),
            },
        )
        .await?;
    assert_eq!(
        custody(&fixture, second.id()).await?,
        ("atRecipient".into(), false)
    );

    // B reconnects: its in-flight push is owed to the new connection, under
    // the item's own call identity.
    let mut reconnected = Socket::open(&fixture, B, SERVICE_B).await?;
    let pushed = reconnected.push_envelope(first.id()).await?;
    assert_eq!(pushed.call.operation.as_uuid(), first.id().as_uuid());
    assert_eq!(pushed.call.step.as_str(), "push");
    reconnected
        .answer(
            &fixture,
            pushed.frame,
            PushResult::Accepted {
                accepted_at: fixture.io.now(),
            },
        )
        .await?;
    assert_eq!(
        custody(&fixture, first.id()).await?,
        ("atRecipient".into(), false)
    );
    fixture.close().await
}

/// A Fleet metadata mutation from A's installation: it appends one notice to
/// every feed reader's stream.
async fn mutate(fixture: &Fixture) -> TestResult {
    let answer = fixture
        .call(
            "session",
            "central.fleet-metadata",
            Value::Null,
            json!({"origin":{"installation":SERVICE_A,"machine":"backpressure fixture"},"request":fleet_admission::FleetRequest {
                request_id: message_board::RequestId::generate(),
                // A fresh workspace and name each time: every create commits.
                workspace: durable_steps::Uuid::now_v7().to_string().try_into()?,
                actor: message_board::RecordedActor::Human {
                    human_id: "backpressure fixture".to_owned().try_into()?,
                },
                acting_for: None,
                protocol_version: message_board::ProtocolVersion::CURRENT,
                operation: fleet_admission::FleetOperation::WorkspaceCreate {
                    name: format!("notice {}", durable_steps::Uuid::now_v7()).try_into()?,
                    description: "appends one project-list hint".to_owned().try_into()?,
                },
            }}),
        )
        .await?;
    if answer.pointer("/decision/kind") != Some(&json!("accepted")) {
        return Err(format!("the notice-producing mutation was refused: {answer}").into());
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The counts name which part of the reconnect's frames arrived."
)]
#[tokio::test]
async fn a_full_request_window_and_a_full_notice_step_reach_a_reconnecting_reader_without_a_drop()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    // A reads the feed, so every metadata mutation owes it a notice.
    fixture
        .call(
            "session",
            "central.fleet-read",
            Value::Null,
            json!({"kind":"feed","value":{"after":0}}),
        )
        .await?;
    // While A is offline, more than a window of pushes and more than a step of
    // notices are owed to it.
    let window = usize::try_from(collaboration_central_core::OWED_REQUEST_WINDOW)?;
    let step = collaboration_central_core::NOTICE_PUSH_LIMIT;
    for _ in 0..=window {
        submit(&fixture, &envelope(A, SERVICE_A)?).await?;
    }
    for _ in 0..=step {
        mutate(&fixture).await?;
    }
    // Connecting stages its catch-up before the test reads a single frame.
    let mut reader = Socket::open(&fixture, A, SERVICE_A).await?;
    let (mut pushes, mut notices) = (0, 0);
    while reader.queued_commands() > 0 {
        match reader.next_command().await? {
            SocketCommand::Close(reason) => {
                return Err(format!("a healthy reader was dropped: {reason:?}").into());
            }
            SocketCommand::Frame(bytes) => {
                match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                    ServerFrame::Request {
                        body: ServerRequest::Crate(ServerCall::Push { .. }),
                        ..
                    } => pushes += 1,
                    ServerFrame::Notice { .. } => notices += 1,
                    _ => {}
                }
            }
        }
    }
    assert_eq!(pushes, window, "the reconnect did not send a full window");
    // The catch-up after the Welcome carries a full notice step beside the
    // window: more than a 64-frame queue holds.
    assert!(notices >= step, "only {notices} notices reached the reader");
    fixture.close().await
}
