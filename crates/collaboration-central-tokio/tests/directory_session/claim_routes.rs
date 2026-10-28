//! Public library preconditions, then real normal-session lease decisions and repair.
//! The setup admission gates remain the existing arranger's explicit stand-ins.
use super::*;
use serde::Deserialize;
use sqlx_core::{query::query, row::Row};
use sqlx_turso::sqlx::{ConnectOptions, Connection};
use std::num::NonZeroU64;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn imported_claim_grants_once_and_the_normal_alarm_acknowledges_its_publication()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    let server = support::start(root.path(), &jwks.url).await?;
    let mut machine = support::admit(&server).await?;
    support::create_graph(
        &mut machine.socket,
        &WorkspaceId::generate(),
        &BoardId::generate(),
        &[database],
    )
    .await?;
    machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    server.shutdown().await?;

    // Public ImportTask creates Unheld; actual seed publication initializes its physical Copy.
    registry_setup::import_seeded(root.path(), configuration::registry_config()?, database).await?;
    let server = support::start(root.path(), &jwks.url).await?;
    let mut machine = support::connect(&server).await?;
    let proof = async {
        let attempt = RequestId::new(durable_steps::Uuid::now_v7())?;
        let granted = claim(&mut machine.socket, database, attempt).await?;
        let expected_start = ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(1));
        let HolderChangeAnswer::Granted {
            epoch,
            expires_at,
            start,
        } = &granted
        else {
            return Err("Claim grant disappeared".into());
        };
        assert_eq!(*epoch, Epoch::new(2)?);
        assert_eq!(*start, expected_start);
        assert_eq!(
            claim(&mut machine.socket, database, attempt).await?,
            granted
        );
        assert_eq!(
            claim(
                &mut machine.socket,
                database,
                RequestId::new(durable_steps::Uuid::now_v7())?
            )
            .await?,
            HolderChangeAnswer::Refused(HolderChangeRefusal::AlreadyHeld {
                holder: PeerId::try_from(PEER.to_owned())?
            })
        );
        // A settlement can update Fleet without acknowledging the separate publication.
        // Observe the latter's own durable acknowledgement before any shutdown/reopen repair.
        wait_publication(root.path(), database, Epoch::new(2)?, 1).await?;
        assert_directory_holder(&mut machine.socket, database, *expires_at).await?;
        match support::answer(
            &mut machine.socket,
            OperationRequest::Replication(ReplicationRequest::CopyStatus { database }),
        )
        .await?
        {
            OperationAnswer::Replication(ReplicationAnswer::CopyStatus(
                CopyStatusAnswer::Status(status),
            )) => {
                assert_eq!(status.database, database);
                assert_eq!(status.fence, Epoch::new(2)?);
                assert_eq!(status.acked, expected_start);
            }
            other => {
                return Err(format!(
                    "granted Claim lost its real Copy status: {}",
                    serde_json::to_string(&other)?
                )
                .into());
            }
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    proof?;
    closed?;
    stopped?;
    keys_stopped?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn new_registration_is_published_without_reopening_or_forcing_an_alarm()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = support::start(root.path(), &jwks.url).await?;
    let mut machine = support::admit(&server).await?;
    let proof = async {
        let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
        support::create_graph(
            &mut machine.socket,
            &WorkspaceId::generate(),
            &BoardId::generate(),
            &[database],
        )
        .await?;
        let registered = register(
            &mut machine.socket,
            database,
            RequestId::new(durable_steps::Uuid::now_v7())?,
        )
        .await?;
        let RegistrationAnswer::Registered {
            lease,
            replayed: false,
        } = registered
        else {
            return Err("fresh normal registration was not newly granted".into());
        };
        assert_eq!(lease.epoch, Epoch::new(1)?);
        assert_eq!(lease.holder, PeerId::try_from(PEER.to_owned())?);
        wait_publication(root.path(), database, Epoch::new(1)?, 1).await?;
        assert_directory_holder(&mut machine.socket, database, lease.expires_at).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    proof?;
    closed?;
    stopped?;
    keys_stopped?;
    Ok(())
}

#[tokio::test]
async fn retired_precedes_cached_registration_and_cached_claim_refusals() -> ConformanceResult<()> {
    retirement_precedence(ExpectedRetirement::Retired).await
}

#[tokio::test]
async fn retiring_precedes_cached_registration_but_replays_an_earlier_claim_refusal()
-> ConformanceResult<()> {
    retirement_precedence(ExpectedRetirement::Retiring).await
}

#[derive(Clone, Copy)]
enum ExpectedRetirement {
    Retired,
    Retiring,
}

async fn retirement_precedence(expected: ExpectedRetirement) -> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let config = configuration::registry_config()?;
    let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    let old_claim = RequestId::new(durable_steps::Uuid::now_v7())?;
    let old_registration = RequestId::new(durable_steps::Uuid::now_v7())?;
    let server = support::start(root.path(), &jwks.url).await?;
    let mut machine = support::admit(&server).await?;
    support::create_graph(
        &mut machine.socket,
        &WorkspaceId::generate(),
        &BoardId::generate(),
        &[database],
    )
    .await?;
    assert_eq!(
        claim(&mut machine.socket, database, old_claim).await?,
        HolderChangeAnswer::Refused(HolderChangeRefusal::NotFound)
    );
    machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    server.shutdown().await?;
    registry_setup::import_unheld(root.path(), config, database).await?;
    let server = support::start(root.path(), &jwks.url).await?;
    let mut machine = support::connect(&server).await?;
    let cached = RegistrationAnswer::Refused(RegistrationRefusal::NotWriter { current: None });
    assert_eq!(
        register(&mut machine.socket, database, old_registration).await?,
        cached
    );
    assert_eq!(
        register(&mut machine.socket, database, old_registration).await?,
        cached
    );
    machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    server.shutdown().await?;
    let (retirement, retired) = registry_setup::retire_imported_copy(
        root.path(),
        config,
        database,
        Epoch::new(match expected {
            ExpectedRetirement::Retired => 1,
            ExpectedRetirement::Retiring => 2,
        })?,
    )
    .await?;
    if matches!(expected, ExpectedRetirement::Retiring) {
        assert!(
            matches!(retired, RetirementAnswer::Refused(RetirementRefusal::CopyMismatch { fence, acked })
            if fence == Epoch::new(1)? && acked == ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(0)))
        );
    } else {
        assert!(matches!(retired, RetirementAnswer::Retired { .. }));
    }
    let server = support::start(root.path(), &jwks.url).await?;
    let mut machine = support::connect(&server).await?;
    let proof = async {
        assert_eq!(
            register(&mut machine.socket, database, old_registration).await?,
            RegistrationAnswer::Refused(RegistrationRefusal::Retired)
        );
        let fresh = if matches!(expected, ExpectedRetirement::Retiring) {
            HolderChangeAnswer::Refused(HolderChangeRefusal::TransferInProgress {
                transfer: TransferId::new(retirement.as_uuid())?,
            })
        } else {
            HolderChangeAnswer::Refused(HolderChangeRefusal::Retired)
        };
        assert_eq!(
            claim(
                &mut machine.socket,
                database,
                RequestId::new(durable_steps::Uuid::now_v7())?
            )
            .await?,
            fresh
        );
        let repeated = if matches!(expected, ExpectedRetirement::Retiring) {
            HolderChangeAnswer::Refused(HolderChangeRefusal::NotFound)
        } else {
            HolderChangeAnswer::Refused(HolderChangeRefusal::Retired)
        };
        assert_eq!(
            claim(&mut machine.socket, database, old_claim).await?,
            repeated
        );
        if matches!(expected, ExpectedRetirement::Retiring) {
            wait_publication(root.path(), database, Epoch::new(1)?, 2).await?;
            let entries =
                support::directory(&mut machine.socket, DirectoryQuery::default()).await?;
            let entry = entries
                .iter()
                .find(|entry| entry.project.as_str() == database.to_string())
                .ok_or("Retiring publication omitted the project")?;
            assert!(
                matches!(
                    &entry.lease_side,
                    DirectorySide::Known(LeaseMetadata {
                        lease: DirectoryLease::Unheld,
                        ..
                    })
                ),
                "Retiring must publish no holder or expiry while Claim remains blocked"
            );
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let closed = machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    proof?;
    closed?;
    stopped?;
    keys_stopped?;
    Ok(())
}

async fn register(
    socket: &mut reqwest_websocket::WebSocket,
    database: DatabaseId,
    attempt: RequestId,
) -> ConformanceResult<RegistrationAnswer> {
    match support::answer(
        socket,
        OperationRequest::Lease(LeaseRequest::Register(Registration {
            attempt,
            database,
            writer: PeerId::try_from(PEER.to_owned())?,
            origin: Origin::Empty,
        })),
    )
    .await?
    {
        OperationAnswer::Lease(LeaseAnswer::Register(answer)) => Ok(answer),
        other => Err(format!(
            "normal Register returned {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}

async fn claim(
    socket: &mut reqwest_websocket::WebSocket,
    database: DatabaseId,
    attempt: RequestId,
) -> ConformanceResult<HolderChangeAnswer> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match support::answer(
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
                        "normal Claim returned {}",
                        serde_json::to_string(&other)?
                    )
                    .into());
                }
            }
        }
    })
    .await?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ObservedPublicationVersion {
    epoch: Epoch,
    incarnation: NonZeroU64,
    change: NonZeroU64,
}

async fn wait_publication(
    root: &Path,
    database: DatabaseId,
    epoch: Epoch,
    expected_change: u64,
) -> ConformanceResult<()> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("registry.db"))
        .read_only(true)
        .connect()
        .await?;
    let observed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let row = query::<sqlx_turso::Turso>(
                "SELECT version FROM registry_lease_acknowledgements WHERE project_id=?",
            )
            .bind(database.to_string())
            .fetch_optional(&mut connection)
            .await?;
            if let Some(row) = row {
                // A narrow typed observation of the private version, never a write or a second owner.
                let version: ObservedPublicationVersion =
                    serde_json::from_str(&row.try_get::<String, _>("version")?)?;
                if version.epoch == epoch && version.change.get() == expected_change {
                    assert!(version.incarnation.get() > 0);
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    let closed = connection.close().await;
    observed??;
    closed?;
    Ok(())
}

async fn assert_directory_holder(
    socket: &mut reqwest_websocket::WebSocket,
    database: DatabaseId,
    expected_expiry: SyncTime,
) -> ConformanceResult<()> {
    let entries = support::directory(socket, DirectoryQuery::default()).await?;
    let entry = entries
        .iter()
        .find(|entry| entry.project.as_str() == database.to_string())
        .ok_or("publication omitted the project")?;
    assert!(
        matches!(&entry.lease_side, DirectorySide::Known(LeaseMetadata {
        lease: DirectoryLease::Held { holder, expires_at, .. }, ..
    }) if holder.as_str() == SERVICE && expires_at.timestamp_millis() == expected_expiry.unix_millis())
    );
    Ok(())
}
