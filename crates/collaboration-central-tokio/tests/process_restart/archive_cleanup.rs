//! A refused Claim's exact settlement and archive completion survive process loss.
use super::*;
use collaboration_central_conformance::{
    AdmittedPeerSocket, ObjectReplyFaultObservation, RuntimeControlRequest, owner_assertion,
};
use collaboration_central_core::{
    ClaimReservationOutcome, ClaimReservationState, ConformanceBoardState, RegistryTime,
    StepControl, StepControlReply,
};
use fleet_sync_protocol::{
    ClaimSettlement, DatabaseId, HolderChangeAnswer, HolderChangeRefusal, HolderChangeRequest,
    TransferId,
};
use reqwest_websocket::WebSocket;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};

#[path = "archive_cleanup/prepare.rs"]
mod prepare;
#[path = "archive_cleanup/verify.rs"]
mod verify;

const READER: &str = "550e8400-e29b-41d4-a716-446655440703";
const READER_PEER: &str = "019f0000-0000-7000-8000-000000007031";
const READER_CREDENTIAL: &str = "archive cleanup reader";

#[derive(Clone, Copy)]
enum CleanupLoss {
    Dispatch,
    Acknowledgement,
}

#[derive(Clone, Debug, PartialEq)]
struct FleetView {
    records: Vec<Record>,
    feed: Vec<Record>,
    head: u64,
}

struct CleanupInput {
    database: DatabaseId,
    at: RegistryTime,
    original: ClaimSettlement,
    call: Value,
    operation: Value,
    archive: FleetRequest,
    decision: FleetRequestResult,
    before: FleetView,
    settled_before: Option<FleetView>,
}
struct PreparedCleanup {
    input: CleanupInput,
    claimant: AdmittedPeerSocket,
    reader: AdmittedPeerSocket,
}
struct SavedJournal {
    input: Vec<u8>,
    digest: Vec<u8>,
    kind: String,
}

#[tokio::test]
async fn refused_claim_cleanup_recovers_after_process_death_before_dispatch()
-> ConformanceResult<()> {
    run_cleanup_restart(CleanupLoss::Dispatch).await
}
#[tokio::test]
async fn refused_claim_cleanup_recovers_after_real_settlement_acknowledgement_loss()
-> ConformanceResult<()> {
    run_cleanup_restart(CleanupLoss::Acknowledgement).await
}

async fn run_cleanup_restart(loss: CleanupLoss) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let database = imported_board_fixture::arrange_imported_unheld_board(
        root.path(),
        ConformanceBoardState::Open,
    )
    .await?;
    let jwks = JwksFixtureServer::start().await?;
    let first = ServerProcess::start(root.path(), &jwks.url).await?;
    let prepared = prepare::prepare_cleanup(&first.base, database, loss).await;
    let crashed = first.crash().await;
    crashed?;
    let prepared = match prepared {
        Ok(value) => value,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    drop(prepared.claimant);
    drop(prepared.reader);
    let input = prepared.input;
    let registry_path = root.path().join("registry.db");
    if operation(&registry_path, &input.operation).await?.0 != "running" {
        jwks.shutdown().await?;
        return Err("original refused-settlement debt completed before process death".into());
    }
    let journal = read_journal(root.path(), &input.operation).await?;
    if serde_json::from_slice::<HolderChangeRequest>(&journal.input)? != input.original.change {
        jwks.shutdown().await?;
        return Err("stopped sender did not retain the exact original Claim input".into());
    }
    let ledger = read_ledger(root.path()).await?;
    let controls = [
        RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command: RuntimeControl::SetTime {
                now: RegistryTime(input.at.0 + chrono::TimeDelta::seconds(6)),
            },
        },
        RuntimeControlRequest {
            address: ObjectAddress::Registry,
            command: RuntimeControl::HoldNextDispatch {
                destination: ObjectAddress::Fleet,
            },
        },
    ];
    let second = ServerProcess::start_with_controls(root.path(), &jwks.url, &controls).await?;
    let proof = verify::verify_cleanup(&second.base, root.path(), &input, &journal, loss).await;
    let _ = control(
        &second.base,
        ObjectAddress::Registry,
        RuntimeControl::ResumeDispatch,
    )
    .await;
    let stopped = second.stop().await;
    let keys_stopped = jwks.shutdown().await;
    proof?;
    stopped?;
    keys_stopped?;
    if read_ledger(root.path()).await? != ledger {
        return Err(
            "archive cleanup or replay changed the full original Fleet request ledger".into(),
        );
    }
    println!(
        "ARCHIVE_CLEANUP_RESTART_CASE exact_settlement_finalization_ledger_and_terminal_debt passed"
    );
    Ok(())
}

async fn read_fleet(socket: &mut WebSocket) -> ConformanceResult<FleetView> {
    let snapshot = creation_scenarios::operation_answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::FleetSnapshot,
            cursor: None,
        }),
    )
    .await?;
    let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::FleetSnapshot {
        records,
        next: None,
        snapshot_at,
        ..
    })) = snapshot
    else {
        return Err("cleanup fixture did not fit one complete Fleet snapshot".into());
    };
    let records = records
        .into_iter()
        .map(|record| record.decode::<Record>())
        .collect::<Result<Vec<_>, _>>()?;
    let page = creation_scenarios::operation_answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::FleetRecords,
            cursor: Some(BoardReadCursor::Fleet(FleetPosition::try_from(0)?)),
        }),
    )
    .await?;
    let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::FleetFeed(
        FleetFeedRead::Page {
            records: feed,
            through,
            head,
            ..
        },
    ))) = page
    else {
        return Err("cleanup fixture did not return its full Fleet feed".into());
    };
    if through != head || snapshot_at != head {
        return Err("cleanup read did not preserve one complete Fleet snapshot/feed prefix".into());
    }
    Ok(FleetView {
        records,
        feed: feed
            .into_iter()
            .map(|record| record.decode::<Record>())
            .collect::<Result<Vec<_>, _>>()?,
        head: head.get(),
    })
}

fn finalized(before: &FleetView, at: RegistryTime) -> ConformanceResult<FleetView> {
    let mut records = before.records.clone();
    let mut board = None;
    let mut workspace = None;
    for record in &mut records {
        let position = match record {
            Record::Board(value) => {
                if value.state != BoardState::Archiving {
                    return Err("Active reservation did not keep the actual board Archiving".into());
                }
                value.state = BoardState::Archived;
                before.head + 1
            }
            Record::Workspace(value) => {
                if value.state != WorkspaceState::Archiving {
                    return Err(
                        "Active reservation did not keep the actual workspace Archiving".into(),
                    );
                }
                value.state = WorkspaceState::Archived;
                before.head + 2
            }
            _ => continue,
        };
        let envelope = record.envelope_mut();
        let RecordPlacement::Fleet(entry) = &mut envelope.position else {
            return Err("archive finalization lost Fleet placement".into());
        };
        entry.position = FleetPosition::try_from(position)?;
        envelope.request = None;
        envelope.time = RecordTime::Router(at.0);
        // The existing actor and verified origin deliberately remain intact.
        match record {
            Record::Board(_) => board = Some(record.clone()),
            Record::Workspace(_) => workspace = Some(record.clone()),
            _ => {}
        }
    }
    let mut feed = before.feed.clone();
    feed.push(board.ok_or("archive fixture has no board")?);
    feed.push(workspace.ok_or("archive fixture has no workspace")?);
    Ok(FleetView {
        records,
        feed,
        head: before.head + 2,
    })
}

async fn read_ledger(root: &std::path::Path) -> ConformanceResult<Vec<String>> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .read_only(true)
        .connect()
        .await?;
    let result = async {
        let rows = query::<sqlx_turso::Turso>(
            "SELECT payload FROM fleet_request_decisions ORDER BY request_id",
        )
        .fetch_all(&mut connection)
        .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
            rows.into_iter()
                .map(|row| row.try_get("payload"))
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
    .await;
    let closed = connection.close().await;
    let rows = result?;
    closed?;
    Ok(rows)
}

async fn read_journal(
    root: &std::path::Path,
    operation: &Value,
) -> ConformanceResult<SavedJournal> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("registry.db"))
        .read_only(true)
        .connect()
        .await?;
    let result = async {
        let row = query::<sqlx_turso::Turso>(
            "SELECT input,input_digest,kind FROM _fleet_sync_operations WHERE operation_id=?",
        )
        .bind(
            operation
                .as_str()
                .ok_or("settlement operation is not text")?,
        )
        .fetch_one(&mut connection)
        .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(SavedJournal {
            input: row.try_get("input")?,
            digest: row.try_get("input_digest")?,
            kind: row.try_get("kind")?,
        })
    }
    .await;
    let closed = connection.close().await;
    let row = result?;
    closed?;
    Ok(row)
}

async fn exact_ledger_decision(
    base: &str,
    request: message_board::RequestId,
) -> ConformanceResult<RequestLedgerEntry> {
    let result = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{base}/admin/v1/conformance/fleet-receipt"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&StepControl::Read {
            request_id: request,
        })
        .send()
        .await?
        .error_for_status()?
        .json::<Result<StepControlReply, durable_steps::StoreError>>()
        .await??;
    match result {
        StepControlReply::Read(Some(entry)) => Ok(*entry),
        _ => Err("original archive ledger entry is absent".into()),
    }
}
