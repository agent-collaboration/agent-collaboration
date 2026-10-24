//! A new Registry process gates publication; Fleet rejects delayed old grants.
use super::*;
use collaboration_central_conformance::{RuntimeControlRequest, owner_assertion};
use collaboration_central_core::{
    ClaimReservationOutcome, ClaimReservationState, ConformanceBoardState, RegistryIncarnation,
    RegistryTime, StepConformance, StepControl, StepControlReply,
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{
    DirectoryEntry, DirectoryLease, DirectoryLeaseState, DirectorySide, LeaseMetadata,
};
use fleet_sync_protocol::{
    ClaimReservation, ClaimSettlement, DatabaseId, Epoch, GateAnswer, HolderChangeAnswer,
    HolderChangeRefusal, HolderChangeRequest, LeaseAnswer, LeaseRequest, PeerId, RenewAnswer,
    RenewRequest, RenewResult, SettleOutcome, TransferId,
};
use reqwest_websocket::WebSocket;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};
use std::sync::Arc;

struct InitialGrant {
    database: DatabaseId,
    attempt: TransferId,
    at: RegistryTime,
    entry: DirectoryEntry,
    projection: Value,
}

#[tokio::test]
async fn a_new_registry_run_gates_publication_and_fences_delayed_grants() -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let database = imported_board_fixture::arrange_imported_unheld_board(
        root.path(),
        ConformanceBoardState::Open,
    )
    .await?;
    let jwks = JwksFixtureServer::start().await?;
    let first = ServerProcess::start(root.path(), &jwks.url).await?;
    let prepared = prepare_grant(&first.base, root.path(), database).await;
    let crashed = first.crash().await;
    crashed?;
    let (at, attempt, entry) = match prepared {
        Ok(value) => value,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    let projection = read_projection(root.path(), database).await?;
    let initial = InitialGrant {
        database,
        attempt,
        at,
        entry,
        projection,
    };
    let controls = [RuntimeControlRequest {
        address: ObjectAddress::Registry,
        command: RuntimeControl::HoldNextDispatch {
            destination: ObjectAddress::Fleet,
        },
    }];
    let second = ServerProcess::start_with_controls(root.path(), &jwks.url, &controls).await?;
    let physical = verify_startup_gate(&second.base, root.path(), &initial).await;
    // Release our exact hold even when a deliberately wrong build fails its oracle.
    let _ = control(
        &second.base,
        ObjectAddress::Registry,
        RuntimeControl::ResumeDispatch,
    )
    .await;
    let stopped = second.stop().await;
    let keys_stopped = jwks.shutdown().await;
    physical?;
    stopped?;
    keys_stopped?;
    println!("LEASE_ORDER_RESTART_STAGE physical_gate_and_renewal passed");

    // This segment uses the stopped owning Fleet store, actual Core decoder
    // and transactions. It is separate from the physical transport segment.
    verify_delayed_grant_in_stopped_fleet(root.path(), &initial).await?;
    println!("LEASE_ORDER_RESTART_CASE physical_gate_and_store_boundary_fence passed");
    Ok(())
}

async fn prepare_grant(
    base: &str,
    root: &std::path::Path,
    database: DatabaseId,
) -> ConformanceResult<(RegistryTime, TransferId, DirectoryEntry)> {
    let at = RegistryTime(
        chrono::DateTime::from_timestamp_millis(chrono::Utc::now().timestamp_millis())
            .ok_or("Registry time is outside timestamp range")?,
    );
    control(
        base,
        ObjectAddress::Registry,
        RuntimeControl::SetTime { now: at },
    )
    .await?;
    let mut holder = enroll_admitted_peer(
        &ConformanceClient::new(base)?,
        SERVICE,
        PEER,
        CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let proof = async {
        let request = fleet_sync_protocol::RequestId::new(Uuid::now_v7())?;
        let attempt = TransferId::new(request.as_uuid())?;
        let answer = imported_claim_scenarios::claim(&mut holder.socket, database, request).await?;
        let expected = HolderChangeAnswer::Granted {
            epoch: Epoch::new(2)?,
            expires_at: runtime_primitives::SyncTime::from_unix_millis(
                at.0.timestamp_millis() + 14_400_000,
            ),
            start: fleet_sync_protocol::ReplicationPosition::new(
                Epoch::new(1)?,
                fleet_sync_protocol::CommitSequence::new(1),
            ),
        };
        if answer != expected {
            return Err(format!("initial real Claim lost its exact grant: {answer:?}").into());
        }
        let state = reservation(base, database, attempt).await?;
        if state
            != Some(ClaimReservationState::Settled {
                outcome: ClaimReservationOutcome::Granted,
            })
        {
            return Err(format!(
                "real Claim did not settle its exact original attempt Granted: {state:?}"
            )
            .into());
        }
        let entry = read_directory(&mut holder.socket, database).await?;
        let projection = read_projection(root, database).await?;
        if entry.lease_side != expected_lease(at, at, &projection)? {
            return Err(
                "Claim's real Directory projection differs from its committed grant".into(),
            );
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((at, attempt, entry))
    }
    .await;
    let closed = holder
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let result = proof?;
    closed?;
    Ok(result)
}

fn expected_lease(
    as_of: RegistryTime,
    grant_at: RegistryTime,
    projection: &Value,
) -> ConformanceResult<DirectorySide<LeaseMetadata>> {
    Ok(DirectorySide::Known(LeaseMetadata {
        lease: DirectoryLease::Held {
            epoch: Epoch::new(2)?,
            holder: SERVICE.to_owned().try_into()?,
            state: DirectoryLeaseState::Held,
            expires_at: grant_at.0 + chrono::TimeDelta::hours(4),
        },
        // The committed source projection supplies the version oracle; the
        // Directory response under test never supplies its own expected stamp.
        lease_version: serde_json::from_value(json!({
            "epoch": projection.pointer("/snapshot/revision/epoch"),
            "incarnation": projection.get("incarnation"),
            "change": projection.pointer("/snapshot/revision/change"),
        }))?,
        as_of: as_of.0,
    }))
}

async fn verify_startup_gate(
    base: &str,
    root: &std::path::Path,
    initial: &InitialGrant,
) -> ConformanceResult<()> {
    let observed = control(
        base,
        ObjectAddress::Registry,
        RuntimeControl::AwaitHeldDispatch,
    )
    .await?;
    let DispatchFaultObservation::Held {
        destination,
        request,
    } = observed.dispatch
    else {
        return Err("Registry startup did not reach its armed transport barrier".into());
    };
    let call: Value = serde_json::from_slice(&request)?;
    if destination != ObjectAddress::Fleet
        || call.get("kind") != Some(&json!("central.lease-incarnation"))
    {
        return Err(
            format!("Registry published before acquiring a new run incarnation: {call}").into(),
        );
    }
    let old: RegistryIncarnation = serde_json::from_value(
        initial
            .projection
            .get("incarnation")
            .cloned()
            .ok_or("old projection omitted incarnation")?,
    )?;
    if call.pointer("/request/atLeast") != Some(&json!(old.value())) {
        return Err("startup allocation lost its last persisted incarnation floor".into());
    }
    let mut holder = registration::reconnect(base).await?;
    let proof = async {
        if read_directory(&mut holder, initial.database).await? != initial.entry {
            return Err(
                "startup changed the Directory before Fleet allocated its incarnation".into(),
            );
        }
        let now = RegistryTime(initial.at.0 + chrono::TimeDelta::seconds(1));
        control(
            base,
            ObjectAddress::Registry,
            RuntimeControl::SetTime { now },
        )
        .await?;
        let renewal = RenewRequest {
            attempt: fleet_sync_protocol::RequestId::new(Uuid::now_v7())?,
            database: initial.database,
            epoch: Epoch::new(2)?,
        };
        let reply = creation_scenarios::operation_answer(
            &mut holder,
            OperationRequest::Lease(LeaseRequest::Renew(renewal)),
        )
        .await?;
        let OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {
            recorded,
            current,
            transfer_in_progress,
            replayed,
        })) = reply
        else {
            return Err("incarnation allocation blocked the authoritative renewal answer".into());
        };
        if recorded
            != (RenewResult {
                epoch: Epoch::new(2)?,
                expires_at: runtime_primitives::SyncTime::from_unix_millis(
                    now.0.timestamp_millis() + 14_400_000,
                ),
            })
            || current.database != initial.database
            || current.epoch != Epoch::new(2)?
            || current.holder != PeerId::try_from(PEER.to_owned())?
            || current.expires_at != recorded.expires_at
            || replayed
            || transfer_in_progress.is_some()
        {
            return Err(
                "renewal lost its exact committed epoch/holder/expiry while publication was gated"
                    .into(),
            );
        }
        if read_directory(&mut holder, initial.database).await? != initial.entry {
            return Err("renewal bypassed the held incarnation gate and published early".into());
        }
        control(
            base,
            ObjectAddress::Registry,
            RuntimeControl::ResumeDispatch,
        )
        .await?;
        tokio::time::timeout(RECOVERY_BOUND, async {
            loop {
                let projection = read_projection(root, initial.database).await?;
                let expected = expected_lease(now, now, &projection)?;
                let entry = read_directory(&mut holder, initial.database).await?;
                if entry.lease_side == expected {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| "current renewal did not publish after the actual incarnation allocation")??;
        if reservation(base, initial.database, initial.attempt).await?
            != Some(ClaimReservationState::Settled {
                outcome: ClaimReservationOutcome::Granted,
            })
        {
            return Err("restart or renewal changed the original settled Claim attempt".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = holder
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

async fn read_directory(
    socket: &mut WebSocket,
    database: DatabaseId,
) -> ConformanceResult<DirectoryEntry> {
    let page = creation_scenarios::operation_answer(
        socket,
        OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::Directory {
                query: BoardPayload::encode(&fleet_admission::DirectoryQuery::default())?,
            },
            cursor: None,
        }),
    )
    .await?;
    let OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Directory {
        entries,
        ..
    })) = page
    else {
        return Err("lease-order query returned another public read".into());
    };
    entries
        .into_iter()
        .map(|entry| entry.decode::<DirectoryEntry>())
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|entry| entry.project.as_str() == database.to_string())
        .ok_or_else(|| "public Directory omitted the imported project".into())
}

pub(super) async fn reservation(
    base: &str,
    database: DatabaseId,
    attempt: TransferId,
) -> ConformanceResult<Option<ClaimReservationState>> {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{base}/admin/v1/conformance/fleet-receipt"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&StepControl::ReadClaimReservation { database, attempt })
        .send()
        .await?
        .error_for_status()?
        .json::<Result<StepControlReply, StoreError>>()
        .await??;
    match response {
        StepControlReply::ClaimReservation(state) => Ok(state),
        _ => Err("Claim reservation observer returned another result".into()),
    }
}

async fn read_projection(root: &std::path::Path, database: DatabaseId) -> ConformanceResult<Value> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .read_only(true)
        .connect()
        .await?;
    let result = async {
        let row = query::<sqlx_turso::Turso>(
            "SELECT lease_projection FROM project_directory WHERE project_id=?",
        )
        .bind(database.to_string())
        .fetch_one(&mut connection)
        .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(serde_json::from_str(
            &row.try_get::<String, _>("lease_projection")?,
        )?)
    }
    .await;
    let closed = connection.close().await;
    let value = result?;
    closed?;
    Ok(value)
}

async fn verify_delayed_grant_in_stopped_fleet(
    root: &std::path::Path,
    initial: &InitialGrant,
) -> ConformanceResult<()> {
    let current = read_projection(root, initial.database).await?;
    let old: RegistryIncarnation = serde_json::from_value(
        initial
            .projection
            .get("incarnation")
            .cloned()
            .ok_or("old incarnation absent")?,
    )?;
    let new: RegistryIncarnation = serde_json::from_value(
        current
            .get("incarnation")
            .cloned()
            .ok_or("current incarnation absent")?,
    )?;
    if new <= old || current.pointer("/snapshot/revision/change") != Some(&json!(2)) {
        return Err(
            "reopened Registry did not publish the actual newer incarnation/change2".into(),
        );
    }
    let fleet = native_fleet::Fixture::open(root).await?;
    let proof = async {
        let allocation = fleet_call(
            &fleet,
            "central.lease-incarnation",
            json!({"atLeast":new.value()}),
        )
        .await?;
        let fence: RegistryIncarnation = serde_json::from_value(allocation)?;
        if fence <= new {
            return Err("actual Fleet allocation failed to advance the witness fence".into());
        }
        let pending = TransferId::new(Uuid::now_v7())?;
        let change = HolderChangeRequest::Claim {
            attempt: pending,
            database: initial.database,
            claimant: PeerId::try_from(PEER.to_owned())?,
        };
        let reserve = fleet_call(
            &fleet,
            <fleet_sync_lease::ClaimReservation as durable_steps::CallTarget>::KIND,
            serde_json::to_value(ClaimReservation {
                change: change.clone(),
            })?,
        )
        .await?;
        if serde_json::from_value::<GateAnswer>(reserve)? != GateAnswer::Reserved {
            return Err(
                "fresh exact attempt did not become Active before stale grant delivery".into(),
            );
        }
        let before = projection_from_fleet(&fleet, initial.database).await?;
        let delayed = fleet_call(
            &fleet,
            "central.claim-granted",
            json!({"attempt":pending,"projection":initial.projection}),
        )
        .await?;
        if delayed != json!({"kind":"fenced","value":{"fence":fence}}) {
            return Err(format!(
                "below-fence delayed ClaimGranted was not refused Fenced: {delayed}"
            )
            .into());
        }
        let observed = StepConformance::new(
            Arc::new(collaboration_central_tokio::TokioStorageHost::new(
                fleet.object.clone(),
            )),
            Arc::new(runtime_primitives::UuidV7Ids),
            runtime_primitives::SyncTime::from_unix_millis(initial.at.0.timestamp_millis()),
        )
        .execute(StepControl::ReadClaimReservation {
            database: initial.database,
            attempt: pending,
        })
        .await?;
        if !matches!(
            observed,
            StepControlReply::ClaimReservation(Some(ClaimReservationState::Active))
        ) {
            return Err("fenced grant settled the fresh Active attempt".into());
        }
        if projection_from_fleet(&fleet, initial.database).await? != before || before != current {
            return Err("fenced grant changed the current committed lease projection".into());
        }
        let settled = fleet_call(
            &fleet,
            <fleet_sync_lease::ClaimSettlement as durable_steps::CallTarget>::KIND,
            serde_json::to_value(ClaimSettlement {
                change,
                outcome: SettleOutcome::Refused {
                    reason: HolderChangeRefusal::PreviewStale,
                },
            })?,
        )
        .await?;
        if settled != json!(null) {
            return Err(format!(
                "exact reservation cleanup changed its SettlementRecorded result: {settled}"
            )
            .into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = fleet.objects.close().await;
    proof?;
    closed?;
    Ok(())
}

async fn projection_from_fleet(
    fleet: &native_fleet::Fixture,
    database: DatabaseId,
) -> ConformanceResult<Value> {
    Ok(fleet
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT lease_projection FROM project_directory WHERE project_id=?",
                )
                .bind(database.to_string())
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                serde_json::from_str(
                    &row.try_get::<String, _>("lease_projection")
                        .map_err(StoreError::backend)?,
                )
                .map_err(StoreError::backend)
            })
        })
        .await?)
}

async fn fleet_call(
    fleet: &native_fleet::Fixture,
    kind: &str,
    request: Value,
) -> ConformanceResult<Value> {
    let bytes=fleet.runtime.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"registry"},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"fleet"},"destinationFace":"fleet", "identity":{"operation":Uuid::now_v7(),"step":"lease_order_witness"},
        "kind":kind,"target":null,"request":request,
    }))?).await.map_err(|error|format!("real stopped-Fleet {kind} call: {error:?}"))?;
    Ok(serde_json::from_slice(&bytes)?)
}
