//! Old import/Claim, stale Preview, transfer, other-project and unread-answer order.
use super::*;

pub(super) async fn claim(socket: &mut WebSocket, database: DatabaseId) -> ConformanceResult<()> {
    let attempt = RequestId::new(Uuid::now_v7())?;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            match creation::operation_answer(
                socket,
                OperationRequest::Lease(LeaseRequest::Claim { database, attempt }),
            )
            .await?
            {
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Pending {
                    ..
                })) => {}
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Granted {
                    epoch,
                    start,
                    ..
                })) if epoch == Epoch::new(2)? && start == position(1, 3)? => {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                other => {
                    return Err(format!(
                        "old normal-state import Claim did not grant exact2/start1:3: {}",
                        serde_json::to_string(&other)?
                    )
                    .into());
                }
            }
        }
    })
    .await?
}
pub(super) async fn preview(
    http: &reqwest::Client,
    route: &str,
) -> ConformanceResult<TakeoverPreview> {
    let answer =
        owner_registry_scenarios::post(http, &format!("{route}/preview"), &Value::Null).await?;
    Ok(serde_json::from_value(
        answer
            .get("preview")
            .cloned()
            .ok_or("normal-state Preview omitted native confirmation")?,
    )?)
}
impl NormalRedeployState {
    pub(in super::super) async fn prepare(
        base: &str,
        [project, second]: [DatabaseId; 2],
    ) -> ConformanceResult<Self> {
        creation::pin_registry_clock(base, RegistryTime("2030-03-17T17:46:40Z".parse()?)).await?;
        let client = ConformanceClient::new(base)?;
        let old_session = SessionId::new(Uuid::now_v7())?;
        let target_session = SessionId::new(Uuid::now_v7())?;
        let mut old = enroll_admitted_peer(
            &client,
            OLD_SERVICE,
            OLD_PEER,
            OLD_CREDENTIAL,
            &old_session.to_string(),
        )
        .await?;
        let mut target = enroll_admitted_peer(
            &client,
            TARGET_SERVICE,
            TARGET_PEER,
            TARGET_CREDENTIAL,
            &target_session.to_string(),
        )
        .await?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()?;
        let proof = async {
            claim(&mut old.socket, project).await?;
            claim(&mut old.socket, second).await?;
            transport::acked(
                &mut old.socket,
                &piece(project, position(1, 3)?, position(2, 1)?,
                    vec![row(4, "a-4")?, row(1, "a-1")?, delete(2)?])?,
                position(2, 1)?,
            ).await?;
            let route = format!("{base}/admin/v1/projects/{project}/takeover");
            let stale_preview = preview(&http, &route).await?;
            transport::acked(
                &mut old.socket,
                &piece(project, position(2, 1)?, position(2, 2)?, vec![row(5, "a-5")?])?,
                position(2, 2)?,
            ).await?;
            let stale_confirmation = json!({
                "transferId": Uuid::now_v7(), "toInstallation": TARGET_SERVICE,
                "reason": "normal-state stale Preview", "preview": stale_preview,
            });
            let initial = owner_registry_scenarios::post(&http, &route, &stale_confirmation).await?;
            let answer = owner_registry_scenarios::complete(
                &http, &route, &stale_confirmation, initial,
            ).await?;
            if answer != json!({"kind":"refused","value":{"kind":"previewStale"}}) {
                return Err(format!("normal-state lost its old stale Preview refusal: {answer}").into());
            }
            transport::exact(
                &mut old.socket, project, position(2, 2)?,
                &[(1, "a-1"), (3, "imported-3"), (4, "a-4"), (5, "a-5")],
            ).await?;
            transport::acked(
                &mut old.socket,
                &piece(project, position(2, 2)?, position(2, 3)?, vec![row(6, "a-6")?])?,
                position(2, 3)?,
            ).await?;
            println!("NORMAL_REDEPLOY_STAGE old_import_Claim2_head1_3_stale_Preview_rows_and_still_writable2_3 passed");
            let takeover = json!({
                "transferId": Uuid::now_v7(), "toInstallation": TARGET_SERVICE,
                "reason": "normal-state actual Takeover", "preview": preview(&http, &route).await?,
            });
            let initial = owner_registry_scenarios::post(&http, &route, &takeover).await?;
            let granted = owner_registry_scenarios::complete(&http, &route, &takeover, initial).await?;
            let expected = json!({"kind":"granted","value":{"newEpoch":4,"start":{"epoch":2,"sequence":3}}});
            if granted != expected || owner_registry_scenarios::post(&http, &route, &takeover).await? != expected {
                return Err(format!("old normal-state Takeover oracle expected4/start2:3: {granted}").into());
            }
            let stale = piece(project, position(2, 3)?, position(2, 4)?, vec![row(7, "stale")?, delete(1)?])?;
            let answer = creation::operation_answer(
                &mut old.socket,
                OperationRequest::Replication(ReplicationRequest::Piece(stale.clone())),
            ).await?;
            if !matches!(answer,
                OperationAnswer::Replication(ReplicationAnswer::Refused(
                    PieceRefusal::StaleEpoch { newer, ended_at }
                )) if newer == Epoch::new(4)? && ended_at == position(2, 3)?
            ) {
                return Err("old normal-state lost exact stale4/cutoff2:3".into());
            }
            transport::acked(
                &mut old.socket,
                &piece(second, position(1, 3)?, position(2, 1)?, vec![row(4, "second-4")?])?,
                position(2, 1)?,
            ).await?;
            transport::acked(
                &mut target.socket,
                &piece(project, position(2, 3)?, position(4, 1)?, vec![row(7, "b-7")?])?,
                position(4, 1)?,
            ).await?;
            transport::exact(
                &mut target.socket, project, position(4, 1)?,
                &[(1, "a-1"), (3, "imported-3"), (4, "a-4"), (5, "a-5"), (6, "a-6"), (7, "b-7")],
            ).await?;
            transport::exact(
                &mut old.socket, second, position(2, 1)?,
                &[(1, "imported-1"), (2, "imported-2"), (3, "imported-3"), (4, "second-4")],
            ).await?;
            let unanswered = piece(second, position(2, 1)?, position(2, 2)?, vec![row(5, "second-5")?, delete(2)?])?;
            creation::send(
                &mut old.socket,
                OperationRequest::Replication(ReplicationRequest::Piece(unanswered.clone())),
            ).await?;
            SinkExt::close(&mut old.socket).await?;
            let mut replacement = transport::reconnect(base, OLD_CREDENTIAL, old_session).await?;
            let replay = async {
                transport::acked(&mut replacement, &unanswered, position(2, 2)?).await?;
                transport::acked(
                    &mut replacement,
                    &piece(second, position(2, 2)?, position(2, 3)?, vec![row(2, "second-2-again")?])?,
                    position(2, 3)?,
                ).await?;
                transport::acked(&mut replacement, &unanswered, position(2, 2)?).await?;
                transport::exact(
                    &mut replacement, second, position(2, 3)?,
                    &[(1, "imported-1"), (2, "second-2-again"), (3, "imported-3"), (4, "second-4"), (5, "second-5")],
                ).await?;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
            }
            .await;
            let closed = replacement.close(reqwest_websocket::CloseCode::Normal, None).await;
            replay?;
            closed?;
            println!("NORMAL_REDEPLOY_STAGE old_stale_cutoff_other_project_and_unread_original_ack_after_update passed");
            let in_flight = piece(project, position(4, 1)?, position(4, 2)?, vec![row(8, "b-8")?])?;
            let in_flight_frame = creation::send(
                &mut target.socket,
                OperationRequest::Replication(ReplicationRequest::Piece(in_flight.clone())),
            ).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((takeover, stale, in_flight, in_flight_frame))
        }
        .await;
        match proof {
            Ok((takeover, stale, in_flight, in_flight_frame)) => Ok(Self {
                project,
                takeover,
                stale,
                in_flight,
                in_flight_frame,
                original_target: target.socket,
                old_session,
                target_session,
            }),
            Err(error) => {
                let _ = old
                    .socket
                    .close(reqwest_websocket::CloseCode::Normal, None)
                    .await;
                let _ = target
                    .socket
                    .close(reqwest_websocket::CloseCode::Normal, None)
                    .await;
                Err(error)
            }
        }
    }
}
