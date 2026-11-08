//! Public library preconditions, then real normal-session lease decisions and repair.
//! The setup admission gates remain the existing arranger's explicit stand-ins.
use super::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn imported_claim_grants_once_and_its_committed_fleet_view_is_immediately_readable()
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
        // The hook commits the view with the lease; no second owner's ACK or
        // forced alarm is needed before the next real Directory read.
        assert_directory_version(&mut machine.socket, database, Epoch::new(2)?, 1).await?;
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
        assert_directory_version(&mut machine.socket, database, Epoch::new(1)?, 1).await?;
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
    let database = DatabaseId::try_from(composed_fleet_fixture::PROJECT.to_owned())?;
    let old_claim = RequestId::new(durable_steps::Uuid::now_v7())?;
    let old_registration = RequestId::new(durable_steps::Uuid::now_v7())?;
    let server = support::start(root.path(), &jwks.url).await?;
    let mut machine = support::admit(&server).await?;
    assert_eq!(
        claim(&mut machine.socket, database, old_claim).await?,
        HolderChangeAnswer::Refused(HolderChangeRefusal::NotFound)
    );
    machine
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    server.shutdown().await?;
    // Existing conformance setup supplies an archived parent board, then runs
    // the actual registered ImportTask/hook and physical project seed.
    let composed =
        composed_fleet_fixture::Fixture::open(root.path(), composed_fleet_fixture::NOW).await?;
    let owner = collaboration_central_core::OwnerIdentity::from_access(
        collaboration_central_core::AccessCaller::Owner {
            email: "fixture@example.test".into(),
        },
    )?;
    let position = composed
        .runtime
        .conformance_imported_unheld(
            database,
            owner,
            collaboration_central_core::ConformanceBoardState::Archived,
        )
        .await?;
    assert_eq!(
        position,
        ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(1))
    );
    composed.close().await?;
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
    let server = support::start(root.path(), &jwks.url).await?;
    let retirement = RequestId::new(durable_steps::Uuid::now_v7())?;
    let retired: collaboration_central_admin_protocol::OwnerRetirementAnswer =
        reqwest::Client::builder()
            .no_proxy()
            .build()?
            .post(format!(
                "{}/admin/v1/projects/{database}/retire",
                server.base_url()
            ))
            .header(
                "Cf-Access-Jwt-Assertion",
                collaboration_central_conformance::owner_assertion()?,
            )
            .json(&collaboration_central_admin_protocol::OwnerRetirement {
                request_id: retirement,
                expect_fence: Epoch::new(match expected {
                    ExpectedRetirement::Retired => 1,
                    ExpectedRetirement::Retiring => 2,
                })?,
                expect_acked: position,
            })
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
    if matches!(expected, ExpectedRetirement::Retiring) {
        assert!(
            matches!(retired, collaboration_central_admin_protocol::OwnerRetirementAnswer::Refused(RetirementRefusal::CopyMismatch { fence, acked })
            if fence == Epoch::new(1)? && acked == position)
        );
    } else {
        assert!(matches!(
            retired,
            collaboration_central_admin_protocol::OwnerRetirementAnswer::Retired { .. }
        ));
    }
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
            assert_directory_version(&mut machine.socket, database, Epoch::new(1)?, 2).await?;
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

async fn assert_directory_version(
    socket: &mut reqwest_websocket::WebSocket,
    database: DatabaseId,
    epoch: Epoch,
    change: u64,
) -> ConformanceResult<()> {
    let entries = support::directory(socket, DirectoryQuery::default()).await?;
    let entry = entries
        .iter()
        .find(|entry| entry.project.as_str() == database.to_string())
        .ok_or("committed Fleet view omitted its project")?;
    let DirectorySide::Known(metadata) = &entry.lease_side else {
        return Err("successful lease answer preceded its Fleet view commit".into());
    };
    assert_eq!(metadata.lease_version.epoch().get(), epoch.value());
    assert_eq!(metadata.lease_version.change(), change);
    assert!(metadata.lease_version.incarnation().value() > 0);
    Ok(())
}
