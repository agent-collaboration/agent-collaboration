//! Direct status input, actual lease authority, native peer selection and injected deadlines.
//! No positive recipient ACK is fabricated; receiving-store acceptance remains a higher gate.
use super::{
    composed_fleet_fixture::*,
    fleet_claim_adapter_fixture::prepare_fleet,
    fleet_mail_dispatch::{CAP, Socket},
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_central_core::CoreHost;
use durable_steps::{
    AtomicStore, CallId, OperationId, StoreError, Task,
    storage::{CallReach, JournalStorage, OperationExecution, StepState},
};
use fleet_mail_delivery::{PublicationRef, PublicationTarget, PublishStatus, StatusStream};
use fleet_protocol::{
    ClientFrame, DeliveryAnswer, DeliveryRequest, DeliveryStatus, FrameId, ItemId, OperationAnswer,
    OperationRequest, ServerCall, ServerFrame, ServerRequest, StatusCursor, StatusPublication,
    StatusVersion, Submitter,
};
use fleet_tokio_objects::TokioObject;
use runtime_primitives::Clock;
use serde_json::json;

type Publisher = PublishStatus<TokioObject<CoreHost<Io>>, sqlx_turso::Turso, BoardDeliveryTypes>;
type Acceptance = <BoardDeliveryTypes as fleet_protocol::DeliveryTypes>::Acceptance;
type Publication = StatusPublication<Acceptance>;

async fn admit(fixture: &Fixture) -> TestResult<(OperationId, Publication)> {
    let publication = Publication {
        item: ItemId::new(durable_steps::Uuid::now_v7())?,
        status: DeliveryStatus::Sent,
        version: StatusVersion::new(1),
        cancel: None,
    };
    let input = publication.clone();
    let now = fixture.io.now();
    let operation = fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                StatusStream::<_, BoardDeliveryTypes>::new(
                    scope,
                    Submitter::Project(PROJECT.to_owned().try_into().map_err(StoreError::backend)?),
                    SERVICE_A
                        .to_owned()
                        .try_into()
                        .map_err(StoreError::backend)?,
                )
                .append_and_admit::<Publisher>(&input, now)
                .await
                .map_err(fleet_mail_delivery::delivery_rollback)?;
                let mut publications = Vec::new();
                for id in scope.due(now).await? {
                    let record = scope
                        .operation(id)
                        .await?
                        .ok_or(StoreError::MissingOperation { id })?;
                    if record.kind == Publisher::KIND {
                        publications.push(id);
                    }
                }
                let [id] = publications.as_slice() else {
                    return Err(StoreError::backend(
                        "expected exactly one admitted publication",
                    ));
                };
                Ok(*id)
            })
        })
        .await?;
    Ok((operation, publication))
}

async fn require_pending(
    fixture: &Fixture,
    operation: OperationId,
    reach: CallReach,
    reason: Option<&str>,
) -> TestResult {
    let (reference, step) = fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let record = scope
                    .operation(operation)
                    .await?
                    .ok_or(StoreError::MissingOperation { id: operation })?;
                let OperationExecution::Running { input, .. } = record.execution else {
                    return Err(StoreError::backend(
                        "unanswered Project publication became terminal",
                    ));
                };
                let reference: PublicationRef<message_board::ServiceId> =
                    serde_json::from_slice(&input).map_err(StoreError::backend)?;
                // SDK's named local read precedes its named publish call.
                let step = scope
                    .step(operation, 1)
                    .await?
                    .ok_or_else(|| StoreError::backend("publish call disappeared"))?;
                Ok((reference, step))
            })
        })
        .await?;
    assert!(
        matches!(reference.target,PublicationTarget::Project {project,..} if project.to_string()==PROJECT),
        "journal cached a machine instead of the Project target"
    );
    assert_eq!(step.name.as_str(), "publish");
    let StepState::CallPending { attempt, .. } = step.state else {
        return Err("unanswered publication did not retain its pending call".into());
    };
    assert_eq!(attempt.reach, reach);
    assert_eq!(
        attempt
            .retry_reason
            .as_ref()
            .map(|retry| retry.code.as_str()),
        reason
    );
    Ok(())
}

async fn publication(socket: &mut Socket) -> TestResult<(CallId, Publication)> {
    loop {
        match socket.next().await? {
            ServerFrame::Request {
                body: ServerRequest::Crate(ServerCall::PublishStatus { call, publication }),
                ..
            } => return Ok((call, publication)),
            ServerFrame::Notice { .. } => {}
            other => {
                return Err(format!(
                    "Project status received another frame: {}",
                    serde_json::to_string(&other)?
                )
                .into());
            }
        }
    }
}

async fn machine_fetch_is_empty(fixture: &Fixture, socket: &mut Socket) -> TestResult {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    fixture.runtime.on_frame(socket.binding,serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
        frame,body:OperationRequest::Delivery(DeliveryRequest::StatusFetch {
            call:serde_json::from_value(json!({"operation":durable_steps::Uuid::now_v7(),"step":"machine_only_status"}))?,after:StatusCursor::new(0),limit:10,
        }),
    })?).await?;
    loop {
        match socket.next().await? {
            ServerFrame::Answer {
                frame: actual,
                body: OperationAnswer::Delivery(DeliveryAnswer::StatusFetch(page)),
            } => {
                assert_eq!(actual, frame);
                assert_eq!(page.through, StatusCursor::new(0));
                assert!(
                    page.events.is_empty(),
                    "Machine fetch leaked the Project stream"
                );
                return Ok(());
            }
            ServerFrame::Notice { .. } => {}
            other => {
                return Err(format!(
                    "Machine status fetch received another frame: {}",
                    serde_json::to_string(&other)?
                )
                .into());
            }
        }
    }
}

#[derive(Clone, Copy)]
enum UnavailableLease {
    Missing,
    Expired,
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Missing or expired authority must remain retryable without routing to a captured service or exposing Project events through Machine fetch."
)]
async fn absent_or_expired_project_holder_keeps_durable_publication_pending_without_a_frame()
-> TestResult {
    for unavailable in [UnavailableLease::Missing, UnavailableLease::Expired] {
        let root = tempfile::tempdir()?;
        let fixture = match unavailable {
            UnavailableLease::Missing => Fixture::open(root.path(), NOW).await?,
            UnavailableLease::Expired => prepare_fleet(root.path(), false).await?,
        };
        let mut machine = Socket::open(&fixture, A, SERVICE_A).await?;
        if matches!(unavailable, UnavailableLease::Expired) {
            fixture.io.advance(60_000);
        }
        let (operation, _) = admit(&fixture).await?;
        let report = tokio::time::timeout(CAP, fixture.runtime.on_alarm()).await??;
        assert!(report.failed.is_empty(), "{report:?}");
        assert!(report.retried.contains(&operation));
        assert_eq!(
            machine.queued_commands(),
            0,
            "Project publication fell back to a machine without Held authority"
        );
        require_pending(
            &fixture,
            operation,
            CallReach::NoEffect,
            Some("projectHolderUnavailable"),
        )
        .await?;
        machine_fetch_is_empty(&fixture, &mut machine).await?;
        fixture.close().await?;
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "An unanswered Project publication cannot finish; its exact original call must select a successor afresh on retry."
)]
async fn silent_project_holder_retries_the_original_call_to_the_real_successor_without_caching_a_peer()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let mut first = Socket::open(&fixture, A, SERVICE_A).await?;
    let mut successor = Socket::open(&fixture, B, SERVICE_B).await?;
    let (operation, expected) = admit(&fixture).await?;
    let mut deadlines = fixture.io.reply_deadlines.subscribe();
    let first_deadline = *deadlines.borrow() + 1;
    let (report, original) = Box::pin(tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let (call, sent) = publication(&mut first).await.map_err(StoreError::backend)?;
            assert_eq!(sent, expected);
            assert_eq!(call.operation, operation);
            assert_eq!(call.step.as_str(), "publish");
            tokio::time::timeout(CAP, deadlines.wait_for(|count| *count >= first_deadline))
                .await
                .map_err(StoreError::backend)?
                .map_err(StoreError::backend)?;
            fixture.io.advance(2_000);
            Ok::<_, StoreError>(call)
        })
    }))
    .await??;
    assert!(report.failed.is_empty(), "{report:?}");
    assert!(report.retried.contains(&operation));
    require_pending(&fixture, operation, CallReach::MayHaveReached, None).await?;
    let preview = fixture.owner(json!({"kind":"preview"})).await?;
    let preview = preview
        .pointer("/value/value/preview")
        .cloned()
        .ok_or("actual takeover preview absent")?;
    let answer=fixture.owner(json!({"kind":"takeover","value":{
        "transferId":durable_steps::Uuid::now_v7(),"toInstallation":SERVICE_B,"reason":"Project status retry changes its current holder","preview":preview
    }})).await?;
    assert_eq!(answer.pointer("/value/kind"), Some(&json!("granted")));
    fixture.io.advance(10_000);
    let second_deadline = *deadlines.borrow() + 1;
    let (report, ()) = Box::pin(tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let (call, sent) = publication(&mut successor)
                .await
                .map_err(StoreError::backend)?;
            assert_eq!(call, original);
            assert_eq!(sent, expected);
            tokio::time::timeout(CAP, deadlines.wait_for(|count| *count >= second_deadline))
                .await
                .map_err(StoreError::backend)?
                .map_err(StoreError::backend)?;
            fixture.io.advance(2_000);
            Ok::<_, StoreError>(())
        })
    }))
    .await??;
    assert!(report.failed.is_empty(), "{report:?}");
    assert!(report.retried.contains(&operation));
    require_pending(&fixture, operation, CallReach::MayHaveReached, None).await?;
    while first.queued_commands() > 0 {
        assert!(
            matches!(first.next().await?, ServerFrame::Notice { .. }),
            "a retry selected the previous holder after the successor barrier"
        );
    }
    fixture.close().await
}
