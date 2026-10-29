//! Native process death preserves journaled work and replayable answers.
#![cfg(all(feature = "conformance", unix))]

#[path = "process_restart/archive_cleanup.rs"]
mod archive_cleanup;
#[path = "native_http/support.rs"]
mod configuration;
#[path = "process_restart/copy_staged_parts.rs"]
mod copy_staged_parts;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/creation_scenarios.rs"]
mod creation_scenarios;
#[path = "process_restart/fleet_notice_outbox.rs"]
mod fleet_notice_outbox;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/imported_board_fixture.rs"]
mod imported_board_fixture;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/imported_claim_scenarios.rs"]
mod imported_claim_scenarios;
#[path = "process_restart/lease_publication.rs"]
mod lease_publication;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "fleet_storage/support.rs"]
mod native_fleet;
#[path = "process_restart/registration.rs"]
mod registration;
#[path = "process_restart/registry_notices.rs"]
mod registry_notices;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;
#[path = "process_restart/retirement.rs"]
mod retirement;
#[path = "process_restart/session_resume.rs"]
mod session_resume;
#[path = "process_restart/support.rs"]
mod support;

use board_sync_protocol::*;
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, DispatchFaultObservation, JwksFixtureServer,
    RuntimeControl, enroll_admitted_peer, fixture_policy,
};
use collaboration_central_core::ObjectAddress;
use collaboration_central_tokio::{TokioServer, TokioSettings};
use durable_steps::Uuid;
use fleet_admission::{FleetOperation, FleetRequest, FleetRequestResult};
use fleet_sync_protocol::{Notice, OperationAnswer, OperationRequest, ServerFrame};
use futures_util::StreamExt;
use message_board::*;
use serde_json::{Value, json};
use std::{io::Write, time::Duration};
use support::*;

/// Invoked only by ServerProcess, in a fresh OS process on each start.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "child entry for the process restart tests; requires its isolated root and JWKS"]
async fn native_central_process() -> ConformanceResult<()> {
    let settings = TokioSettings {
        root: std::env::var("CENTRAL_RESTART_ROOT")?.into(),
        jwks_url: std::env::var("CENTRAL_RESTART_JWKS")?,
        access_policy: fixture_policy(),
        registry: configuration::registry_config()?,
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    };
    let controls: Vec<collaboration_central_conformance::RuntimeControlRequest> =
        serde_json::from_str(&std::env::var("CENTRAL_RESTART_CONTROLS")?)?;
    let now = std::env::var("CENTRAL_RESTART_NOW")?;
    let server = if !now.is_empty() {
        TokioServer::start_with_clock_and_controls(
            settings,
            std::sync::Arc::new(RestartClock(now.parse()?)),
            controls,
        )
        .await?
    } else if controls.is_empty() {
        TokioServer::start(settings).await?
    } else {
        TokioServer::start_with_controls(settings, controls).await?
    };
    println!("CENTRAL_PROCESS_READY {}", server.base_url());
    std::io::stdout().flush()?;
    let startup_alarms: Vec<ObjectAddress> =
        serde_json::from_str(&std::env::var("CENTRAL_RESTART_ALARMS")?)?;
    for address in startup_alarms {
        tokio::time::timeout(
            Duration::from_secs(10),
            server.await_startup_alarm(address.clone()),
        )
        .await??;
        println!(
            "CENTRAL_STARTUP_ALARM_COMPLETED {}",
            serde_json::to_string(&address)?
        );
        std::io::stdout().flush()?;
    }
    tokio::task::spawn_blocking(|| {
        let mut command = String::new();
        std::io::stdin().read_line(&mut command)
    })
    .await??;
    server.shutdown().await?;
    Ok(())
}

struct RestartClock(i64);
impl runtime_primitives::Clock for RestartClock {
    fn now(&self) -> runtime_primitives::SyncTime {
        runtime_primitives::SyncTime::from_unix_millis(self.0)
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn fleet_notice_finishes_once_after_the_server_process_dies_before_dispatch()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let first = ServerProcess::start(root.path(), &jwks.url).await?;
    let client = ConformanceClient::new(&first.base)?;
    let mut reader = enroll_admitted_peer(
        &client,
        SERVICE,
        PEER,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let mut writer = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440702",
        "019f0000-0000-7000-8000-000000007021",
        "restart-writer",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let page = creation_scenarios::operation_answer(
        &mut reader.socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::FleetRecords,
            cursor: Some(BoardReadCursor::Fleet(FleetPosition::try_from(0)?)),
        }),
    )
    .await?;
    assert!(
        matches!(page,OperationAnswer::Consumer(BoardConsumerAnswer::Read(
        BoardReadPage::FleetFeed(FleetFeedRead::Page {head,..}))) if head.get()==0)
    );
    control(
        &first.base,
        ObjectAddress::Fleet,
        RuntimeControl::HoldNextDispatch {
            destination: ObjectAddress::Connection(SERVICE.to_owned().try_into()?),
        },
    )
    .await?;
    let request = FleetRequest {
        request_id: message_board::RequestId::generate(),
        workspace: WorkspaceId::generate(),
        actor: RecordedActor::Human {
            human_id: HumanId::try_from("restart owner".to_owned())?,
        },
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation: FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("restart notice source".to_owned())?,
            description: Description::try_from("committed before process death".to_owned())?,
        },
    };
    let expected = decide(&mut writer.socket, &request).await?;
    assert!(matches!(
        &expected,
        FleetRequestResult::Decided(RequestDecision::Accepted {
            change: ChangeOutcome::Changed,
            ..
        })
    ));
    let held = control(
        &first.base,
        ObjectAddress::Fleet,
        RuntimeControl::AwaitHeldDispatch,
    )
    .await?;
    let DispatchFaultObservation::Held { request: bytes, .. } = held.dispatch else {
        return Err("Fleet did not reach its post-commit dispatch barrier".into());
    };
    let call: Value = serde_json::from_slice(&bytes)?;
    let operation_id = call
        .pointer("/identity/operation")
        .ok_or("notice call omitted its durable identity")?
        .clone();
    let fleet_path = root.path().join("fleet.db");
    let connection_path = root.path().join(format!("connection-{SERVICE}.db"));
    let before = operation(&fleet_path, &operation_id).await?;
    assert_eq!(before.0, "running");
    assert_eq!(before.2, 1);
    assert_eq!(
        read_count(
            &connection_path,
            "SELECT COUNT(*) AS count FROM machine_notices"
        )
        .await?,
        0
    );
    first.crash().await?;
    drop(reader);
    drop(writer);

    let second = ServerProcess::start(root.path(), &jwks.url).await?;
    let client = ConformanceClient::new(&second.base)?;
    let mut reader = enroll_admitted_peer(
        &client,
        SERVICE,
        PEER,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    // No Fleet request, explicit alarm, or retry triggers recovery here. Only
    // the child's startup drive can rearm the persisted Fleet operation.
    tokio::time::timeout(RECOVERY_BOUND, async {
        loop {
            let message = reader
                .socket
                .next()
                .await
                .ok_or("reopened reader closed before notice")??;
            let bytes = match message {
                reqwest_websocket::Message::Binary(bytes) => bytes.to_vec(),
                reqwest_websocket::Message::Text(text) => text.into_bytes(),
                other => return Err(format!("unexpected restart notice frame: {other:?}").into()),
            };
            let frame: ServerFrame<BoardConsumerProtocol> = serde_json::from_slice(&bytes)?;
            if let ServerFrame::Notice {
                body: Notice::Consumer(notice),
                ..
            } = frame
            {
                assert_eq!(notice.position.value(), 1);
                assert_eq!(
                    notice.kind,
                    BoardNotice::FleetFeedAdvanced {
                        through: FleetPosition::try_from(2)?
                    }
                );
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
        }
    })
    .await
    .map_err(|_| "startup did not recover the owed Fleet notice")??;
    let page = creation_scenarios::operation_answer(
        &mut reader.socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::Notices,
            cursor: None,
        }),
    )
    .await?;
    assert!(
        matches!(page,OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Notices(NoticeRead::Page {notices,..}))) if notices.len()==1)
    );
    let mut writer = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440702",
        "019f0000-0000-7000-8000-000000007021",
        "restart-writer",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    assert_eq!(decide(&mut writer.socket, &request).await?, expected);
    // Orderly stop drains the alarm which acknowledged the already observed
    // append. The independent readback then checks terminal work and counts.
    drop(reader);
    drop(writer);
    second.stop().await?;
    let after = operation(&fleet_path, &operation_id).await?;
    assert_eq!(after.0, "completed");
    assert_eq!(after.2, 0, "terminal journal completion compacts its steps");
    assert_eq!(
        read_count(
            &fleet_path,
            "SELECT COUNT(*) AS count FROM _fleet_sync_operations WHERE kind='central.fleet-notice'"
        )
        .await?,
        1
    );
    assert_eq!(
        read_count(&fleet_path, "SELECT COUNT(*) AS count FROM workspaces").await?,
        1
    );
    assert_eq!(
        read_count(
            &connection_path,
            "SELECT COUNT(*) AS count FROM machine_notices"
        )
        .await?,
        1
    );
    jwks.shutdown().await?;
    Ok(())
}

async fn decide(
    socket: &mut reqwest_websocket::WebSocket,
    request: &FleetRequest,
) -> ConformanceResult<FleetRequestResult> {
    let answer = creation_scenarios::operation_answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Fleet(FleetRequestEnvelope {
            request_id: request.request_id.clone(),
            operation: BoardPayload::encode(request)?,
        })),
    )
    .await?;
    let OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
        request_id,
        decision,
    }) = answer
    else {
        return Err("restart mutation returned another operation family".into());
    };
    assert_eq!(request_id, request.request_id);
    Ok(decision.decode()?)
}
