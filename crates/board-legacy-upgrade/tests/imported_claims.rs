//! D's actual owner import must remain readable and claimable by normal sessions.
#[path = "upload_support/central_fixture.rs"]
#[expect(
    dead_code,
    reason = "Shared fixture helpers serve different subsets of integration targets."
)]
mod central_fixture;
mod support;
#[path = "upload_support/mod.rs"]
#[expect(
    dead_code,
    reason = "The shared publication helpers also serve the existing upload route tests."
)]
mod upload_support;

use board_legacy_upgrade::UpgradePublisher;
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_central_admin_protocol::{OwnerRetirement, OwnerRetirementAnswer};
use collaboration_central_conformance::{
    ConformanceClient, JwksFixtureServer, enroll_admitted_peer,
};
use collaboration_central_core::{
    AdminProjectImportAnswer, ClaimReservationState, StepControl, StepControlReply,
};
use fleet_sync_protocol::{
    ClientFrame, CopyStatusAnswer, DatabaseId, Epoch, FrameId, HolderChangeAnswer,
    HolderChangeRefusal, LeaseAnswer, LeaseRequest, OperationAnswer, OperationRequest, PeerId,
    ReplicationAnswer, ReplicationRequest, RequestId, ServerFrame, SnapshotRead, TransferId,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use std::time::Duration;
use support::*;

async fn operation(
    socket: &mut WebSocket,
    body: OperationRequest<BoardConsumerProtocol>,
) -> TestResult<OperationAnswer<BoardConsumerProtocol>> {
    let frame = FrameId::new(uuid::Uuid::now_v7())?;
    socket
        .send(Message::Binary(
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request { frame, body })?
                .into(),
        ))
        .await?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = socket
                .next()
                .await
                .ok_or("session closed before answer")??;
            let bytes = match message {
                Message::Binary(bytes) => bytes.to_vec(),
                Message::Text(text) => text.into_bytes(),
                other => return Err(format!("unexpected session frame: {other:?}").into()),
            };
            match serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(&bytes)? {
                ServerFrame::Answer {
                    frame: actual,
                    body,
                } if actual == frame => return Ok(body),
                ServerFrame::Notice { .. } => {}
                _ => return Err("session answer lost its request correlation".into()),
            }
        }
    })
    .await?
}

async fn claim(
    socket: &mut WebSocket,
    database: DatabaseId,
    attempt: RequestId,
) -> TestResult<HolderChangeAnswer> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match operation(
                socket,
                OperationRequest::Lease(LeaseRequest::Claim { database, attempt }),
            )
            .await?
            {
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Pending {
                    transfer,
                })) if transfer.as_uuid() == attempt.as_uuid() => tokio::task::yield_now().await,
                OperationAnswer::Lease(LeaseAnswer::Claim(answer)) => return Ok(answer),
                other => {
                    return Err(format!(
                        "Claim returned another operation: {}",
                        serde_json::to_string(&other)?
                    )
                    .into());
                }
            }
        }
    })
    .await?
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn imported_copy_is_readable_unheld_then_real_claims_have_one_epoch_and_preserve_replay()
-> TestResult {
    // Arrange: actual shipped-schema transformation and owner HTTP publication.
    // No Registry/Copy bootstrap or stand-in supplies the imported state.
    let prepared = prepare("legacy-complete.sqlite").await?;
    let (_changed_source, changed_path) = fixture_copy("legacy-complete.sqlite")?;
    let changed_source = rusqlite::Connection::open(&changed_path)?;
    changed_source.execute(
        "UPDATE board_messages SET text=text || ' Changed import content.'",
        [],
    )?;
    changed_source.close().map_err(|(_, error)| error)?;
    let changed = prepare_path(&changed_path).await?;
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let observed: TestResult = async {
        let publisher = UpgradePublisher::new(
            reqwest::Url::parse(server.base_url())?,
            central_fixture::owner_client()?,
        );
        publisher.publish(&prepared).await?;
        let report = prepared.report.projects.first().ok_or("project missing")?;
        let project = prepared.projects.first().ok_or("prepared project missing")?;
        let database = report.manifest.database;
        let client = ConformanceClient::new(server.base_url())
            .map_err(|error| error as Box<dyn std::error::Error>)?;
        let first_peer = PeerId::try_from(uuid::Uuid::now_v7().to_string())?;
        let second_peer = PeerId::try_from(uuid::Uuid::now_v7().to_string())?;
        let mut first = enroll_admitted_peer(
            &client, &uuid::Uuid::now_v7().to_string(), &first_peer.to_string(),
            "import-claim-first", &uuid::Uuid::now_v7().to_string(),
        ).await.map_err(|error| error as Box<dyn std::error::Error>)?;
        let mut second = enroll_admitted_peer(
            &client, &uuid::Uuid::now_v7().to_string(), &second_peer.to_string(),
            "import-claim-second", &uuid::Uuid::now_v7().to_string(),
        ).await.map_err(|error| error as Box<dyn std::error::Error>)?;
        let proof: TestResult = async {
            // Act / Assert: the normal reader sees every exact imported page
            // before anyone holds the project, not just a Copy status marker.
            let answer = operation(&mut first.socket, OperationRequest::Replication(
                ReplicationRequest::CopyStatus { database },
            )).await?;
            assert!(matches!(answer, OperationAnswer::Replication(ReplicationAnswer::CopyStatus(
                CopyStatusAnswer::Status(status)
            )) if status.holder.is_none() && status.fence==Epoch::new(1)? && status.acked==report.manifest.position),
                "the actual owner import must publish an unheld Copy at its exact prefix");
            let mut snapshot = project.database.snapshot().await?;
            let mut cursor = None;
            let pages: TestResult = async {
                loop {
                    let expected = snapshot.page(cursor.clone()).await?;
                    let answer = operation(&mut first.socket, OperationRequest::Replication(
                        ReplicationRequest::SnapshotPage { database, cursor },
                    )).await?;
                    let OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(
                        SnapshotRead::Page(actual)
                    )) = answer else { return Err("unheld imported snapshot was unreadable".into()); };
                    assert_eq!(*actual, expected);
                    cursor=actual.next;
                    if cursor.is_none() { break; }
                }
                Ok(())
            }.await;
            let closed=snapshot.close().await;
            pages?;
            closed?;

            // Two independently enrolled sockets submit competing Claims.
            // PD3 requires the loser to name the winning holder.
            let first_attempt=RequestId::new(uuid::Uuid::now_v7())?;
            let second_attempt=RequestId::new(uuid::Uuid::now_v7())?;
            let (left,right)=tokio::join!(
                claim(&mut first.socket,database,first_attempt),
                claim(&mut second.socket,database,second_attempt),
            );
            let (left,right)=(left?,right?);
            let (winner,attempt,winning_peer,loser) = match (&left,&right) {
                (HolderChangeAnswer::Granted {..}, HolderChangeAnswer::Refused(_)) =>
                    (&mut first.socket,first_attempt,first_peer,&right),
                (HolderChangeAnswer::Refused(_), HolderChangeAnswer::Granted {..}) =>
                    (&mut second.socket,second_attempt,second_peer,&left),
                _ => return Err(format!("Claim race did not have exactly one winner: {left:?}, {right:?}").into()),
            };
            let repeated=claim(winner,database,attempt).await?;
            assert!(matches!(&repeated,HolderChangeAnswer::Granted { epoch,start,.. }
                if *epoch==Epoch::new(2)? && *start==report.manifest.position));
            assert!(repeated==left || repeated==right);
            assert!(matches!(loser,
                HolderChangeAnswer::Refused(HolderChangeRefusal::AlreadyHeld {holder}) if *holder==winning_peer
            ));
            assert_eq!(claim(winner,database,RequestId::new(uuid::Uuid::now_v7())?).await?,
                HolderChangeAnswer::Refused(HolderChangeRefusal::AlreadyHeld {holder:winning_peer}));

            // A completed import replay must retain the claimed fence/holder
            // and all native rows, rather than reseeding the now-held Copy.
            let copy=format!("copy-{database}");
            let before=upload_support::read(root.path(),&copy,upload_support::COPY_SNAPSHOT).await?;
            let replay=publisher.publish(&prepared).await?;
            assert!(replay.projects.iter().all(|result|result.answer==AdminProjectImportAnswer::AlreadyImported));
            assert_eq!(upload_support::read(root.path(),&copy,upload_support::COPY_SNAPSHOT).await?,before);
            let offered = changed.report.projects.first().ok_or("changed project missing")?;
            assert_ne!(offered.digest, report.digest);
            let refusal = publisher.publish(&changed).await.err().ok_or("changed content replaced a claimed import")?;
            assert!(matches!(refusal, board_legacy_upgrade::UpgradeUploadError::ProjectIncomplete {
                answer, ..
            } if *answer==AdminProjectImportAnswer::ImportConflict {
                existing:Some(report.digest.clone()), offered:offered.digest.clone()
            }));
            assert_eq!(upload_support::read(root.path(),&copy,upload_support::COPY_SNAPSHOT).await?,before);
            Ok(())
        }.await;
        let first_closed=first.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        let second_closed=second.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        proof?;
        first_closed?;
        second_closed?;
        Ok(())
    }.await;
    prepared.close().await;
    changed.close().await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    stopped?;
    keys_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    observed
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn retirement_precedes_old_claim_and_completed_import_replay_after_server_restart()
-> TestResult {
    // Arrange: import a genuinely archived legacy board through D's real path.
    // A prior unknown Claim supplies the recorded answer Retired must override.
    let (_source_directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let legacy = rusqlite::Connection::open(&source)?;
    legacy.execute("UPDATE project_boards SET state='archived'", [])?;
    legacy.close().map_err(|(_, error)| error)?;
    let prepared = prepare_path(&source).await?;
    let report = prepared.report.projects.first().ok_or("project missing")?;
    let database = report.manifest.database;
    let first_attempt = RequestId::new(uuid::Uuid::now_v7())?;
    let installation = uuid::Uuid::now_v7().to_string();
    let peer = uuid::Uuid::now_v7().to_string();
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let outcome: TestResult = async {
        for reopened in [false, true] {
            let server = central_fixture::start_server(root.path(), &jwks).await?;
            let proof: TestResult = async {
                let http = central_fixture::owner_client()?;
                let publisher =
                    UpgradePublisher::new(reqwest::Url::parse(server.base_url())?, http.clone());
                let client = ConformanceClient::new(server.base_url())
                    .map_err(|error| error as Box<dyn std::error::Error>)?;
                let mut machine = enroll_admitted_peer(
                    &client,
                    &installation,
                    &peer,
                    "retired-import-claimant",
                    &uuid::Uuid::now_v7().to_string(),
                )
                .await
                .map_err(|error| error as Box<dyn std::error::Error>)?;
                let observed: TestResult = async {
                    if !reopened {
                        assert_eq!(
                            claim(&mut machine.socket, database, first_attempt).await?,
                            HolderChangeAnswer::Refused(HolderChangeRefusal::NotFound)
                        );
                        publisher.publish(&prepared).await?;
                        // Import does not rewrite an existing attempt's answer.
                        assert_eq!(
                            claim(&mut machine.socket, database, first_attempt).await?,
                            HolderChangeAnswer::Refused(HolderChangeRefusal::NotFound)
                        );
                        let retirement = OwnerRetirement {
                            request_id: RequestId::new(uuid::Uuid::now_v7())?,
                            expect_fence: Epoch::new(1)?,
                            expect_acked: report.manifest.position,
                        };
                        let route =
                            format!("{}/admin/v1/projects/{database}/retire", server.base_url());
                        tokio::time::timeout(Duration::from_secs(20), async {
                            loop {
                                let answer: OwnerRetirementAnswer = http
                                    .post(&route)
                                    .json(&retirement)
                                    .send()
                                    .await?
                                    .error_for_status()?
                                    .json()
                                    .await?;
                                match answer {
                                    OwnerRetirementAnswer::Retired { .. } => {
                                        break Ok::<_, Box<dyn std::error::Error>>(());
                                    }
                                    OwnerRetirementAnswer::Pending { request_id, .. }
                                        if request_id == retirement.request_id =>
                                    {
                                        tokio::task::yield_now().await
                                    }
                                    other => {
                                        return Err(format!(
                                            "archived imported retirement failed: {other:?}"
                                        )
                                        .into());
                                    }
                                }
                            }
                        })
                        .await??;
                    }

                    // Act / Assert: the tombstone wins over both a recorded old
                    // Claim refusal and the completed admin import, also cold.
                    assert_eq!(
                        claim(&mut machine.socket, database, first_attempt).await?,
                        HolderChangeAnswer::Refused(HolderChangeRefusal::Retired)
                    );
                    assert_eq!(
                        claim(
                            &mut machine.socket,
                            database,
                            RequestId::new(uuid::Uuid::now_v7())?
                        )
                        .await?,
                        HolderChangeAnswer::Refused(HolderChangeRefusal::Retired)
                    );
                    let refusal = publisher
                        .publish(&prepared)
                        .await
                        .err()
                        .ok_or("retired import replay reported success")?;
                    assert!(matches!(
                        refusal,
                        board_legacy_upgrade::UpgradeUploadError::OwnerFailure {
                            failure:
                                collaboration_central_admin_protocol::OwnerOperationFailure::Retired
                        }
                    ));
                    Ok(())
                }
                .await;
                let closed = machine
                    .socket
                    .close(reqwest_websocket::CloseCode::Normal, None)
                    .await;
                observed?;
                closed?;
                Ok(())
            }
            .await;
            let stopped = server.shutdown().await;
            proof?;
            stopped?;
        }
        Ok(())
    }
    .await;
    prepared.close().await;
    let keys_stopped = jwks.shutdown().await;
    keys_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    outcome
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn a_competing_claim_names_the_active_transfer_until_the_imported_fence_commits() -> TestResult
{
    let prepared = prepare("legacy-complete.sqlite").await?;
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let outcome: TestResult = async {
        let http=central_fixture::owner_client()?;
        let publisher=UpgradePublisher::new(reqwest::Url::parse(server.base_url())?,http.clone());
        publisher.publish(&prepared).await?;
        let report=prepared.report.projects.first().ok_or("project missing")?;
        let database=report.manifest.database;
        let copy=format!("copy-{database}");
        let client=ConformanceClient::new(server.base_url())
            .map_err(|error| error as Box<dyn std::error::Error>)?;
        let winning_peer=PeerId::try_from(uuid::Uuid::now_v7().to_string())?;
        let mut winner=enroll_admitted_peer(&client,&uuid::Uuid::now_v7().to_string(),&winning_peer.to_string(),
            "held-import-fence-first",&uuid::Uuid::now_v7().to_string()).await
            .map_err(|error| error as Box<dyn std::error::Error>)?;
        let mut competitor=enroll_admitted_peer(&client,&uuid::Uuid::now_v7().to_string(),&uuid::Uuid::now_v7().to_string(),
            "held-import-fence-second",&uuid::Uuid::now_v7().to_string()).await
            .map_err(|error| error as Box<dyn std::error::Error>)?;
        let proof: TestResult = async {
            // A real native fault prevents the first fence from committing.
            // The reservation read is the causal pre-grant barrier, not sleep.
            upload_support::execute(root.path(),&copy,
                "CREATE TRIGGER hold_import_fence BEFORE UPDATE ON _fleet_sync_copy BEGIN SELECT RAISE(ABORT,'imported fence held'); END").await?;
            let attempt=RequestId::new(uuid::Uuid::now_v7())?;
            let held:TestResult<HolderChangeAnswer>=async {
                let answer=operation(&mut winner.socket,OperationRequest::Lease(
                    LeaseRequest::Claim {database,attempt})).await?;
                assert!(matches!(answer,OperationAnswer::Lease(LeaseAnswer::Claim(
                    HolderChangeAnswer::Pending {transfer}
                )) if transfer.as_uuid()==attempt.as_uuid()),"blocked first fence granted early");
                let command=StepControl::ReadClaimReservation {
                    database,attempt:TransferId::new(attempt.as_uuid())?,
                };
                let reply:Result<StepControlReply,serde_json::Value>=http
                    .post(format!("{}/admin/v1/conformance/fleet-receipt",server.base_url()))
                    .json(&command).send().await?.error_for_status()?.json().await?;
                let reply=reply.map_err(|error|format!("reservation read failed: {error}"))?;
                assert!(matches!(reply,StepControlReply::ClaimReservation(Some(ClaimReservationState::Active))),
                    "first Claim has no active pre-grant reservation");
                let native=upload_support::read(root.path(),&copy,upload_support::COPY_SNAPSHOT).await?;
                let row:serde_json::Value=serde_json::from_str(native.first().ok_or("native copy missing")?)?;
                let copy_state:serde_json::Value=serde_json::from_str(row.get("copy").and_then(serde_json::Value::as_str).ok_or("copy state missing")?)?;
                assert_eq!(copy_state.pointer("/copy/fence").and_then(serde_json::Value::as_u64),Some(1));
                match operation(&mut competitor.socket,OperationRequest::Lease(LeaseRequest::Claim {
                    database,attempt:RequestId::new(uuid::Uuid::now_v7())?,
                })).await? {
                    OperationAnswer::Lease(LeaseAnswer::Claim(answer))=>Ok(answer),
                    _=>Err("competing Claim returned another family".into()),
                }
            }.await;
            let released=upload_support::execute(root.path(),&copy,"DROP TRIGGER hold_import_fence").await;
            released?;
            let actual=held?;
            let granted=claim(&mut winner.socket,database,attempt).await?;
            assert!(matches!(granted,HolderChangeAnswer::Granted {epoch,start,..}
                if epoch==Epoch::new(2)? && start==report.manifest.position),
                "unblocked first Claim did not resume to its exact grant");
            // Decision 968: an active holder change names its transfer; after
            // grant, a fresh Claim names the existing lease holder instead.
            let expected=HolderChangeAnswer::Refused(HolderChangeRefusal::TransferInProgress {
                transfer:TransferId::new(attempt.as_uuid())?,
            });
            if actual!=expected {
                return Err(format!("held-fence competing Claim must name the active transfer; actual={}",serde_json::to_string(&actual)?).into());
            }
            assert_eq!(claim(&mut competitor.socket,database,RequestId::new(uuid::Uuid::now_v7())?).await?,
                HolderChangeAnswer::Refused(HolderChangeRefusal::AlreadyHeld {holder:winning_peer}));
            Ok(())
        }.await;
        let winner_closed=winner.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        let competitor_closed=competitor.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        proof?;
        winner_closed?;
        competitor_closed?;
        Ok(())
    }.await;
    prepared.close().await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    stopped?;
    keys_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    outcome
}
