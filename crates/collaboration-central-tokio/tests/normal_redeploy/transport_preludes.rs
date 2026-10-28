//! Preserve the old N1/N2 and owner response-loss oracles through public routes.
use super::*;
use board_sync_protocol::{
    BoardClientAnswer, BoardConsumerAnswer, BoardConsumerRequest, BoardPayload, BoardServerRequest,
    ForwardOutcome, ForwardedPost, ForwardedPostKind,
};

pub(super) async fn clean_move(base: &str, database: DatabaseId) -> ConformanceResult<()> {
    let mut old = NormalPeer::enroll(base, "old").await?;
    let mut new = NormalPeer::enroll(base, "new").await?;
    let mut callers = tokio::task::JoinSet::new();
    let proof = async {
        prepare::claim(&mut old.socket, database).await?;
        original_rows(&mut old.socket, database).await?;
        let http = http()?;
        let route = format!("{base}/admin/v1/projects/{database}/move");
        let transfer = TransferId::new(Uuid::now_v7())?;
        let request = json!({"transferId":transfer,"toInstallation":new.installation,"reason":"M3.10 N1 clean move"});
        let (send_http, send_route, send_request) = (http.clone(), route.clone(), request.clone());
        callers.spawn(async move { owner_registry_scenarios::post(&send_http, &send_route, &send_request).await });
        let ServerFrame::Request { frame,
            body: ServerRequest::Crate(ServerCall::Release { request: release, .. }),
        } = next_operation(&mut old.socket).await? else {
            return Err("old N1 clean Move lost its actual holder Release".into());
        };
        if release.database != database || release.epoch != Epoch::new(2)? || release.transfer != transfer {
            return Err("old N1 Release changed its exact project/epoch2/transfer".into());
        }
        let renewed = creation::operation_answer(&mut old.socket,
            OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {
                database, epoch: Epoch::new(2)?, attempt: RequestId::new(Uuid::now_v7())?,
            }))).await?;
        if !matches!(renewed, OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed {transfer_in_progress: Some(actual), current, ..}))
            if actual == transfer && current.epoch == Epoch::new(2)? && current.holder == old.peer) {
            return Err("N1 live Release lost the exact active transfer and old epoch2 authority".into());
        }
        old.socket.send(Message::Binary(serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Answer {
            frame, body: ClientAnswer::Crate(ServerCallAnswer::Release(ReleaseAnswer::Released {final_position: position(1, 3)?})),
        })?.into())).await?;
        let initial = tokio::time::timeout(Duration::from_secs(8), callers.join_next()).await?
            .ok_or("N1 concurrent owner caller vanished")???;
        let granted = owner_registry_scenarios::complete(&http, &route, &request, initial).await?;
        let expected = json!({"kind":"moved","value":{"newEpoch":3,"start":{"epoch":1,"sequence":3}}});
        if granted != expected { return Err(format!("old N1 clean Move lost grant3/start1:3: {granted}").into()); }
        let stale = piece(database, position(1, 3)?, position(2, 1)?, vec![row(1, "stale-left")?, row(9, "stale-right")?])?;
        let answer = creation::operation_answer(&mut old.socket, OperationRequest::Replication(ReplicationRequest::Piece(stale))).await?;
        if !matches!(answer, OperationAnswer::Replication(ReplicationAnswer::Refused(PieceRefusal::StaleEpoch {newer, ended_at}))
            if newer == Epoch::new(3)? && ended_at == position(1, 3)?) {
            return Err("old N1 multi-change stale Piece lost exact newer3/cutoff1:3".into());
        }
        original_rows(&mut old.socket, database).await?;
        transport::acked(&mut new.socket, &piece(database, position(1, 3)?, position(3, 1)?,
            vec![row(1, "moved-1")?, row(4, "moved-4")?])?, position(3, 1)?).await?;
        transport::exact(&mut new.socket, database, position(3, 1)?,
            &[(1, "moved-1"), (2, "imported-2"), (3, "imported-3"), (4, "moved-4")]).await?;
        if owner_registry_scenarios::post(&http, &route, &request).await? != expected {
            return Err("old N1 Move replay changed its exact grant3/start1:3".into());
        }
        println!("NORMAL_REDEPLOY_PRELUDE N1_live_active_transfer_exact_Release_grant3_stale_whole_refusal_new_rows_and_replay passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    callers.abort_all();
    while callers.join_next().await.is_some() {}
    let old_closed = old.close().await;
    let new_closed = new.close().await;
    proof?;
    old_closed?;
    new_closed?;
    Ok(())
}

pub(super) async fn forward_roundtrip(base: &str, database: DatabaseId) -> ConformanceResult<()> {
    let mut holder = NormalPeer::enroll(base, "old").await?;
    let mut origin = NormalPeer::enroll(base, "new").await?;
    let proof = async {
        prepare::claim(&mut holder.socket, database).await?;
        original_rows(&mut holder.socket, database).await?;
        let sender_frame = FrameId::new(Uuid::now_v7())?;
        let project = message_board::ProjectId::try_from(database.to_string())?;
        let post = ForwardedPost { kind: ForwardedPostKind::ThreadPost,
            body: BoardPayload::encode(&json!({"fixture":"M3.10 native forward","requestId":sender_frame}))? };
        origin.socket.send(Message::Binary(serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
            frame: sender_frame, body: OperationRequest::Consumer(BoardConsumerRequest::ForwardedPost {project: project.clone(), post: post.clone()}),
        })?.into())).await?;
        let ServerFrame::Request { frame: holder_frame,
            body: ServerRequest::Consumer(BoardServerRequest::ForwardedPost(received)),
        } = next_operation(&mut holder.socket).await? else {
            return Err("old N2 holder lost its actual forwarded post".into());
        };
        let expected_origin = message_board::ActorOrigin {
            installation: origin.installation.clone(),
            machine: message_board::MachineLabel::try_from("new".to_owned())?,
        };
        if received.project != project || received.post != post || received.origin != expected_origin {
            return Err(format!("old N2 changed exact origin/label/project/post: {received:?}").into());
        }
        let reply = BoardPayload::encode(&json!({"fixture":"M4-holder-admission-stand-in","reply":"native transport response"}))?;
        holder.socket.send(Message::Binary(serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Answer {
            frame: holder_frame, body: ClientAnswer::Consumer(BoardClientAnswer::ForwardedPost(reply.clone())),
        })?.into())).await?;
        let ServerFrame::Answer { frame,
            body: OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(answer)),
        } = next_operation(&mut origin.socket).await? else {
            return Err("old N2 origin lost its actual correlated answer".into());
        };
        if frame != sender_frame || answer != (ForwardOutcome::Answered {reply}) {
            return Err("old N2 changed the original frame or exact opaque holder reply".into());
        }
        original_rows(&mut holder.socket, database).await?;
        println!("NORMAL_REDEPLOY_PRELUDE N2_exact_origin_label_post_correlated_reply_unchanged_Copy passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let holder_closed = holder.close().await;
    let origin_closed = origin.close().await;
    proof?;
    holder_closed?;
    origin_closed?;
    Ok(())
}

pub(super) async fn owner_response_loss(base: &str, database: DatabaseId) -> ConformanceResult<()> {
    let mut old = NormalPeer::enroll(base, "old").await?;
    let mut new = NormalPeer::enroll(base, "new").await?;
    let proof = async {
        prepare::claim(&mut old.socket, database).await?;
        original_rows(&mut old.socket, database).await?;
        let http = http()?;
        let route = format!("{base}/admin/v1/projects/{database}");
        let preview = prepare::preview(&http, &format!("{route}/takeover")).await?;
        let moving = json!({"transferId":Uuid::now_v7(),"toInstallation":new.installation,"reason":"M3.10 denied machine move"});
        let taking = json!({"transferId":Uuid::now_v7(),"toInstallation":new.installation,"reason":"M3.10 denied machine takeover","preview":preview});
        for (operation, request) in [("move", moving), ("takeover", taking)] {
            let response = http.post(format!("{route}/{operation}"))
                .bearer_auth(&old.credential).header("Cf-Access-Jwt-Assertion", machine_assertion(&old.credential)?)
                .json(&request).send().await?;
            let status = response.status().as_u16(); drop(response);
            if !matches!(status, 302 | 401 | 403) {
                return Err(format!("old owner prelude machine {operation} was not denied: {status}").into());
            }
            original_rows(&mut old.socket, database).await?;
        }
        let request = json!({"transferId":Uuid::now_v7(),"toInstallation":new.installation,"reason":"M3.10 unread owner response body","preview":preview});
        let response = http.post(format!("{route}/takeover"))
            .header("Cf-Access-Jwt-Assertion", collaboration_central_conformance::owner_assertion()?)
            .json(&request).send().await?;
        if !response.status().is_success() { return Err("old owner prelude lost successful unread response headers".into()); }
        drop(response);
        let expected = json!({"kind":"granted","value":{"newEpoch":3,"start":{"epoch":1,"sequence":3}}});
        if owner_registry_scenarios::post(&http, &format!("{route}/takeover"), &request).await? != expected
            || owner_registry_scenarios::post(&http, &format!("{route}/takeover"), &request).await? != expected {
            return Err("old owner prelude lost original grant3/start1:3 after unread body".into());
        }
        let stale_preview = json!({"transferId":Uuid::now_v7(),"toInstallation":old.installation,"reason":"M3.10 old preview after holder change","preview":preview});
        if owner_registry_scenarios::post(&http, &format!("{route}/takeover"), &stale_preview).await?
            != json!({"kind":"refused","value":{"kind":"previewStale"}}) {
            return Err("old owner prelude accepted the old-holder/old-epoch Preview".into());
        }
        let mut conflicting = request;
        *conflicting.get_mut("reason").ok_or("owner prelude request omitted its reason")? =
            json!("M3.10 unread owner response body changed");
        if owner_registry_scenarios::post(&http, &format!("{route}/takeover"), &conflicting).await?
            != json!({"kind":"conflictingRequest"}) {
            return Err("old owner prelude lost exact changed-body conflict".into());
        }
        transport::acked(&mut new.socket, &piece(database, position(1, 3)?, position(3, 1)?, vec![row(4, "reply-loss-4")?])?, position(3, 1)?).await?;
        let stale = piece(database, position(1, 3)?, position(2, 1)?, vec![row(9, "stale")?])?;
        let answer = creation::operation_answer(&mut old.socket, OperationRequest::Replication(ReplicationRequest::Piece(stale))).await?;
        if !matches!(answer, OperationAnswer::Replication(ReplicationAnswer::Refused(PieceRefusal::StaleEpoch {newer, ended_at})) if newer == Epoch::new(3)? && ended_at == position(1, 3)?) {
            return Err("old owner prelude lost original stale3/cutoff1:3".into());
        }
        transport::exact(&mut new.socket, database, position(3, 1)?, &[(1, "imported-1"), (2, "imported-2"), (3, "imported-3"), (4, "reply-loss-4")]).await?;
        println!("NORMAL_REDEPLOY_PRELUDE owner_machine_denials_unread_body_exact_grant3_replays_stale_preview_conflict_rows_and_cutoff passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let old_closed = old.close().await;
    let new_closed = new.close().await;
    proof?;
    old_closed?;
    new_closed?;
    Ok(())
}
