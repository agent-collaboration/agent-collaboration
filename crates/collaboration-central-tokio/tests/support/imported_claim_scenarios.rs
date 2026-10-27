//! Real imported Registry/Copy state, then normal owner and session decisions.

use super::creation_scenarios as creation;
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardPayload, BoardReadPage, BoardReadScope,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer, owner_assertion,
};
use collaboration_central_core::{
    ClaimReservationState, ConformanceBoardState, RegistryTime, StepControl, StepControlReply,
};
use durable_steps::Uuid;
use fleet_admission::{
    DirectoryEntry, DirectoryLease, DirectoryQuery, DirectorySide, LeaseMetadata,
};
use fleet_sync_protocol::{
    CommitSequence, CopyStatusAnswer, DatabaseId, Epoch, GateReason, GateReasonCode,
    HolderChangeAnswer, HolderChangeRefusal, LeaseAnswer, LeaseRequest, OperationAnswer,
    OperationRequest, PeerId, ReplicationAnswer, ReplicationPosition, ReplicationRequest,
    RequestId, SnapshotRead, SqlValue, SyncTime, TransferId,
};
use reqwest_websocket::WebSocket;
use serde_json::{Value, json};
use std::time::Duration;

const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440650";
const PEER: &str = "019f0000-0000-7000-8000-000000000650";

pub(super) async fn run_archived(base: &str) -> ConformanceResult<()> {
    let database = DatabaseId::new(Uuid::now_v7())?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let arranged = owner_post(
        &http,
        &format!("{base}/admin/v1/conformance/bootstrap"),
        &json!({
            "fixture": "importedUnheld",
            "command": {"kind": "importedUnheld", "value": {
                "database": database, "boardState": ConformanceBoardState::Archived
            }}
        }),
    )
    .await?;
    if arranged != json!({"epoch": 1, "sequence": 1}) {
        return Err("Archived Claim setup did not publish the actual unretired seed".into());
    }
    archived_claim_is_refused_without_reservation(base, database).await
}

pub(super) async fn archived_claim_is_refused_without_reservation(
    base: &str,
    database: DatabaseId,
) -> ConformanceResult<()> {
    let attempt = RequestId::new(Uuid::now_v7())?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let client = ConformanceClient::new(base)?;
    let mut machine = enroll_admitted_peer(
        &client,
        SERVICE,
        PEER,
        "archived imported claimant",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        if reservation(&http, base, database, attempt).await?.is_some() {
            return Err(
                "Archived Claim already had a reservation before the normal request".into(),
            );
        }
        let expected = HolderChangeAnswer::Refused(HolderChangeRefusal::GateRefused {
            reason: GateReason {
                code: GateReasonCode::new("board_archived")?,
                detail: "claim board is archived".into(),
            },
        });
        let actual = claim(&mut machine.socket, database, attempt).await?;
        if actual != expected {
            return Err(format!(
                "Archived unretired Claim lost its exact board_archived refusal: {actual:?}"
            )
            .into());
        }
        if let Some(stored) = reservation(&http, base, database, attempt).await? {
            return Err(format!("refused Archived Claim wrote a reservation: {stored:?}").into());
        }
        println!("ARCHIVED_CLAIM_CASE exact_board_archived_refusal_without_reservation passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

async fn reservation(
    http: &reqwest::Client,
    base: &str,
    database: DatabaseId,
    attempt: RequestId,
) -> ConformanceResult<Option<ClaimReservationState>> {
    let command = StepControl::ReadClaimReservation {
        database,
        attempt: TransferId::new(attempt.as_uuid())?,
    };
    let response = http
        .post(format!("{base}/admin/v1/conformance/fleet-receipt"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&command)
        .send()
        .await?
        .error_for_status()?
        .json::<Result<StepControlReply, durable_steps::StoreError>>()
        .await??;
    match response {
        StepControlReply::ClaimReservation(reservation) => Ok(reservation),
        other => {
            Err(format!("Claim reservation read returned another control answer: {other:?}").into())
        }
    }
}

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    let database = DatabaseId::try_from("019f0000-0000-7000-8000-000000005650".to_owned())?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    creation::pin_registry_clock(
        base,
        RegistryTime(
            chrono::DateTime::from_timestamp_millis(1_900_000_000_000)
                .ok_or("fixed Claim clock is outside the timestamp range")?,
        ),
    )
    .await?;
    let arranged = owner_post(
        &http,
        &format!("{base}/admin/v1/conformance/bootstrap"),
        &json!({
            "fixture":"importedUnheld",
            "command":{"kind":"importedUnheld","value":{
                "database":database,"boardState":ConformanceBoardState::Open
            }}
        }),
    )
    .await?;
    if arranged != json!({"epoch":1,"sequence":1}) {
        return Err(format!("Open import did not publish its actual seed: {arranged}").into());
    }
    let client = ConformanceClient::new(base)?;
    let mut machine = enroll_admitted_peer(
        &client,
        SERVICE,
        PEER,
        "normal imported claimant",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        // The owner uses the production retire route while the imported board is Open.
        // Its refusal must preserve the actual readable Copy, not merely report Live.
        let before = seeded_page(&mut machine.socket, database).await?;
        let route = format!("{base}/admin/v1/projects/{database}/retire");
        let request = json!({"requestId":Uuid::now_v7(),"expectFence":1,
            "expectAcked":{"epoch":1,"sequence":1}});
        let refusal = owner_post(&http, &route, &request).await?;
        let expected = json!({"kind":"refused","value":{"kind":"gateRefused","value":{
            "reason":{"code":"board_not_archived","detail":"Retirement requires an archived board."}
        }}});
        if refusal != expected {
            return Err(
                format!("Open board lost its exact retirement gate refusal: {refusal}").into(),
            );
        }
        if owner_post(&http, &route, &request).await? != expected {
            return Err("Open-board retirement retry changed its exact gate refusal".into());
        }
        let health = http
            .get(format!("{base}/admin/v1/copies/{database}/health"))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        if health.get("kind") != Some(&json!("live"))
            || seeded_page(&mut machine.socket, database).await? != before
        {
            return Err(
                "refused Open-board retirement changed or wiped the actual seeded Copy".into(),
            );
        }
        println!(
            "IMPORTED_CLAIM_CASE open_board_retirement_exact_refusal_and_seed_unchanged passed"
        );

        let attempt = RequestId::new(Uuid::now_v7())?;
        let granted = claim(&mut machine.socket, database, attempt).await?;
        // The actual Worker configuration grants four hours from its owning clock.
        let expected_expiry = SyncTime::from_unix_millis(1_900_014_400_000);
        let expected_start = ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(1));
        let expected_grant = HolderChangeAnswer::Granted {
            epoch: Epoch::new(2)?,
            expires_at: expected_expiry,
            start: expected_start,
        };
        if granted != expected_grant {
            return Err(format!(
                "imported Claim lost its exact epoch2 grant/start/expiry: {granted:?}"
            )
            .into());
        }
        if claim(&mut machine.socket, database, attempt).await? != expected_grant {
            return Err("same-attempt imported Claim changed its committed grant".into());
        }
        let expected_held = HolderChangeAnswer::Refused(HolderChangeRefusal::AlreadyHeld {
            holder: PeerId::try_from(PEER.to_owned())?,
        });
        if claim(
            &mut machine.socket,
            database,
            RequestId::new(Uuid::now_v7())?,
        )
        .await?
            != expected_held
        {
            return Err("fresh imported Claim lost its actual holder refusal".into());
        }
        await_directory_holder(&mut machine.socket, database, expected_expiry).await?;
        match creation::operation_answer(
            &mut machine.socket,
            OperationRequest::Replication(ReplicationRequest::CopyStatus { database }),
        )
        .await?
        {
            OperationAnswer::Replication(ReplicationAnswer::CopyStatus(
                CopyStatusAnswer::Status(status),
            )) if status.database == database
                && status.fence == Epoch::new(2)?
                && status.acked == expected_start => {}
            other => {
                return Err(format!(
                    "imported Claim lost its actual Copy fence/prefix: {}",
                    serde_json::to_string(&other)?
                )
                .into());
            }
        }
        println!(
            "IMPORTED_CLAIM_CASE exact_grant_replay_fresh_holder_directory_and_copy_fence passed"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

async fn owner_post(http: &reqwest::Client, route: &str, body: &Value) -> ConformanceResult<Value> {
    Ok(http
        .post(route)
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn seeded_page(socket: &mut WebSocket, database: DatabaseId) -> ConformanceResult<Value> {
    match creation::operation_answer(
        socket,
        OperationRequest::Replication(ReplicationRequest::SnapshotPage {
            database,
            cursor: None,
        }),
    )
    .await?
    {
        OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(page)))
            if page.database == database
                && page.position
                    == ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(1))
                && page.table == "owner_retirement_rows"
                && page.next.is_none()
                && page.rows
                    == vec![vec![
                        SqlValue::Text("019f0000-0000-7000-8000-000000005595".into()),
                        SqlValue::Text("fixture imported row".into()),
                    ]] =>
        {
            Ok(serde_json::to_value(page)?)
        }
        other => Err(format!(
            "normal reader lost the exact imported seed: {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}

pub(super) async fn claim(
    socket: &mut WebSocket,
    database: DatabaseId,
    attempt: RequestId,
) -> ConformanceResult<HolderChangeAnswer> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match creation::operation_answer(
                socket,
                OperationRequest::Lease(LeaseRequest::Claim { attempt, database }),
            )
            .await?
            {
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Pending {
                    transfer,
                })) if transfer.as_uuid() == attempt.as_uuid() => tokio::task::yield_now().await,
                OperationAnswer::Lease(LeaseAnswer::Claim(answer)) => return Ok(answer),
                other => {
                    return Err(format!(
                        "normal imported Claim returned {}",
                        serde_json::to_string(&other)?
                    )
                    .into());
                }
            }
        }
    })
    .await?
}

async fn await_directory_holder(
    socket: &mut WebSocket,
    database: DatabaseId,
    expiry: SyncTime,
) -> ConformanceResult<()> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Directory {
                entries,
                ..
            })) = creation::operation_answer(
                socket,
                OperationRequest::Consumer(BoardConsumerRequest::Read {
                    scope: BoardReadScope::Directory {
                        query: BoardPayload::encode(&DirectoryQuery::default())?,
                    },
                    cursor: None,
                }),
            )
            .await?
            else {
                return Err("imported Claim Directory read answered another family".into());
            };
            let entries = entries
                .into_iter()
                .map(|entry| entry.decode::<DirectoryEntry>())
                .collect::<Result<Vec<_>, _>>()?;
            let entry = entries
                .iter()
                .find(|entry| entry.project.as_str() == database.to_string())
                .ok_or("imported Claim publication omitted its project")?;
            match &entry.lease_side {
                DirectorySide::NotYetReported
                | DirectorySide::Known(LeaseMetadata {
                    lease: DirectoryLease::Unheld,
                    ..
                }) => tokio::task::yield_now().await,
                DirectorySide::Known(LeaseMetadata {
                    lease:
                        DirectoryLease::Held {
                            holder, expires_at, ..
                        },
                    ..
                }) if holder.as_str() == SERVICE
                    && expires_at.timestamp_millis() == expiry.unix_millis() =>
                {
                    return Ok(());
                }
                other @ DirectorySide::Known(_) => {
                    return Err(format!(
                        "imported Claim published another holder/expiry: {other:?}"
                    )
                    .into());
                }
            }
        }
    })
    .await?
}
