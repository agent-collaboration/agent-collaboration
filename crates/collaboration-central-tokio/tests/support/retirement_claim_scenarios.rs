//! Cached normal-session decisions are tested against real retirement state.

use super::{
    creation_scenarios as creation, imported_claim_scenarios::claim,
    owner_registry_scenarios as owner,
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardPayload, BoardReadPage, BoardReadScope,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
};
use collaboration_central_core::ConformanceBoardState;
use durable_steps::Uuid;
use fleet_admission::{
    DirectoryEntry, DirectoryLease, DirectoryQuery, DirectorySide, LeaseMetadata,
};
use fleet_sync_protocol::{
    DatabaseId, HolderChangeAnswer, HolderChangeRefusal, OperationAnswer, OperationRequest, PeerId,
    RegistrationAnswer, RegistrationRefusal, RequestId, TransferId,
};
use reqwest_websocket::WebSocket;
use serde_json::json;
use std::time::Duration;

#[derive(Clone, Copy)]
pub(super) enum ExpectedRetirement {
    Retiring,
    Retired,
}

const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440651";
const PEER: &str = "019f0000-0000-7000-8000-000000000651";

pub(super) async fn run(base: &str, state: ExpectedRetirement) -> ConformanceResult<()> {
    let database = DatabaseId::new(Uuid::now_v7())?;
    let old_claim = RequestId::new(Uuid::now_v7())?;
    let old_registration = RequestId::new(Uuid::now_v7())?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let client = ConformanceClient::new(base)?;
    let mut machine = enroll_admitted_peer(
        &client,
        SERVICE,
        PEER,
        "retirement precedence claimant",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        let not_found = HolderChangeAnswer::Refused(HolderChangeRefusal::NotFound);
        if claim(&mut machine.socket, database, old_claim).await? != not_found {
            return Err("pre-import Claim did not cache its exact NotFound refusal".into());
        }
        let arranged = owner::post(
            &http,
            &format!("{base}/admin/v1/conformance/bootstrap"),
            &json!({
                "fixture": "importedUnheld",
                "command": {"kind": "importedUnheld", "value": {
                    "database": database, "boardState": ConformanceBoardState::Archived
                }}
            }),
        ).await?;
        if arranged != json!({"epoch": 1, "sequence": 1}) {
            return Err("retirement precedence setup did not publish its actual seed".into());
        }
        let peer = PeerId::try_from(PEER.to_owned())?;
        let not_writer = RegistrationAnswer::Refused(RegistrationRefusal::NotWriter {
            current: None,
        });
        for _ in 0..2 {
            let cached = creation::register(
                &mut machine.socket, database, peer, old_registration,
            ).await?;
            if cached != not_writer {
                return Err("pre-retirement Register did not cache/replay its exact NotWriter refusal".into());
            }
        }
        let retirement = RequestId::new(Uuid::now_v7())?;
        let expected_fence = match state {
            ExpectedRetirement::Retiring => 2,
            ExpectedRetirement::Retired => 1,
        };
        let request = json!({
            "requestId": retirement, "expectFence": expected_fence,
            "expectAcked": {"epoch": 1, "sequence": 1}
        });
        let route = format!("{base}/admin/v1/projects/{database}/retire");
        let first = owner::post(&http, &route, &request).await?;
        let result = owner::complete(&http, &route, &request, first).await?;
        match state {
            ExpectedRetirement::Retiring => {
                let mismatch = json!({
                    "kind": "refused", "value": {"kind": "copyMismatch", "value": {
                        "fence": 1, "acked": {"epoch": 1, "sequence": 1}
                    }}
                });
                if result != mismatch {
                    return Err(format!(
                        "retirement mismatch did not leave its exact real Copy fence/prefix: {result}"
                    ).into());
                }
            }
            ExpectedRetirement::Retired => {
                if result.get("kind") != Some(&json!("retired")) {
                    return Err(format!(
                        "retirement precedence setup did not finish real retirement: {result}"
                    ).into());
                }
            }
        }
        // Register's current retirement guard outranks its cached NotWriter answer.
        let registration = creation::register(
            &mut machine.socket, database, peer, old_registration,
        ).await?;
        if registration != RegistrationAnswer::Refused(RegistrationRefusal::Retired) {
            return Err("current retirement did not outrank Register's cached NotWriter refusal".into());
        }
        // Retiring preserves an old Claim refusal; Retired overrides that same cache.
        let old_expected = match state {
            ExpectedRetirement::Retiring => not_found,
            ExpectedRetirement::Retired => HolderChangeAnswer::Refused(HolderChangeRefusal::Retired),
        };
        if claim(&mut machine.socket, database, old_claim).await? != old_expected {
            return Err("current retirement lost the required cached-Claim refusal precedence".into());
        }
        let fresh_expected = match state {
            ExpectedRetirement::Retiring => {
                HolderChangeAnswer::Refused(HolderChangeRefusal::TransferInProgress {
                    transfer: TransferId::new(retirement.as_uuid())?,
                })
            }
            ExpectedRetirement::Retired => HolderChangeAnswer::Refused(HolderChangeRefusal::Retired),
        };
        let fresh = RequestId::new(Uuid::now_v7())?;
        if claim(&mut machine.socket, database, fresh).await? != fresh_expected {
            return Err("fresh Claim lost its exact current retirement/transfer refusal".into());
        }
        if matches!(state, ExpectedRetirement::Retiring) {
            await_unheld_directory(&mut machine.socket, database).await?;
            println!("RETIREMENT_CLAIM_CASE retiring_register_guard_old_claim_replay_fresh_transfer_and_unheld_directory passed");
        } else {
            println!("RETIREMENT_CLAIM_CASE retired_overrides_cached_register_cached_claim_and_fresh_claim passed");
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let closed = machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

async fn await_unheld_directory(
    socket: &mut WebSocket,
    database: DatabaseId,
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
                return Err("Retiring Directory read returned another family".into());
            };
            let entries = entries
                .into_iter()
                .map(|entry| entry.decode::<DirectoryEntry>())
                .collect::<Result<Vec<_>, _>>()?;
            let entry = entries
                .iter()
                .find(|entry| entry.project.as_str() == database.to_string())
                .ok_or("Retiring publication omitted its project")?;
            match &entry.lease_side {
                DirectorySide::NotYetReported => tokio::task::yield_now().await,
                DirectorySide::Known(LeaseMetadata {
                    lease: DirectoryLease::Unheld,
                    ..
                }) => return Ok(()),
                other @ DirectorySide::Known(_) => {
                    return Err(format!(
                        "Retiring publication fabricated a holder or expiry: {other:?}"
                    )
                    .into());
                }
            }
        }
    })
    .await?
}
