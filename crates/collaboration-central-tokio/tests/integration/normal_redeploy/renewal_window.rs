//! Original unread renewal replacement and same-socket zero-window preludes.
use super::*;

async fn renew(socket: &mut WebSocket, request: RenewRequest) -> ConformanceResult<RenewAnswer> {
    match creation::operation_answer(
        socket,
        OperationRequest::Lease(LeaseRequest::Renew(request)),
    )
    .await?
    {
        OperationAnswer::Lease(LeaseAnswer::Renew(answer)) => Ok(answer),
        other => Err(format!(
            "old S5 renewal got another answer: {}",
            serde_json::to_string(&other)?
        )
        .into()),
    }
}
pub(super) async fn renewal_replay(base: &str, database: DatabaseId) -> ConformanceResult<()> {
    let mut old = NormalPeer::enroll(base, "old").await?;
    let target = NormalPeer::enroll(base, "new").await?;
    let proof = async {
        prepare::claim(&mut old.socket, database).await?;
        let r1 = RenewRequest {database, epoch: Epoch::new(2)?, attempt: RequestId::new(Uuid::now_v7())?};
        creation::send(&mut old.socket, OperationRequest::Lease(LeaseRequest::Renew(r1.clone()))).await?;
        let replacement = transport::reconnect(base, &old.credential, old.session).await?;
        let unobserved = std::mem::replace(&mut old.socket, replacement);
        let replay = renew(&mut old.socket, r1.clone()).await?;
        let RenewAnswer::Renewed {recorded, replayed: true, ..} = replay else {
            return Err(format!("old S5 original unread R1 was not recorded before replacement: {replay:?}").into());
        };
        let _ = unobserved.close(reqwest_websocket::CloseCode::Normal, None).await;
        if recorded.epoch != Epoch::new(2)? {return Err("old S5 R1 changed its recorded epoch2".into());}
        let first_expiry = recorded.expires_at;
        let fresh = RenewRequest {database, epoch: Epoch::new(2)?, attempt: RequestId::new(Uuid::now_v7())?};
        if !matches!(renew(&mut old.socket, fresh).await?, RenewAnswer::Renewed {recorded, replayed: false, ..} if recorded.epoch == Epoch::new(2)? && recorded.expires_at >= first_expiry) {
            return Err("old S5 fresh R2 did not renew again at/after the first expiry".into());
        }
        let http = http()?;
        let route = format!("{base}/admin/v1/projects/{database}/takeover");
        let preview = prepare::preview(&http, &route).await?;
        let taking = json!({"transferId":Uuid::now_v7(),"toInstallation":target.installation,"reason":"M3.10 staging renewal supersession","preview":preview});
        let initial = owner_registry_scenarios::post(&http, &route, &taking).await?;
        let granted = owner_registry_scenarios::complete(&http, &route, &taking, initial).await?;
        if granted != json!({"kind":"granted","value":{"newEpoch":3,"start":{"epoch":1,"sequence":3}}}) {
            return Err(format!("old S5 supersession lost literal grant3/start1:3: {granted}").into());
        }
        if !matches!(renew(&mut old.socket, r1).await?, RenewAnswer::Renewed {recorded, replayed: true, current, ..}
            if recorded.epoch == Epoch::new(2)? && recorded.expires_at == first_expiry && current.epoch == Epoch::new(3)? && current.holder == target.peer) {
            return Err("old S5 R1 lost its original expiry or superseding current epoch3/target".into());
        }
        let fresh = RenewRequest {database, epoch: Epoch::new(2)?, attempt: RequestId::new(Uuid::now_v7())?};
        if renew(&mut old.socket, fresh).await? != (RenewAnswer::Refused(LeaseRefusal::StaleEpoch {current_epoch: Epoch::new(3)?, epoch_ended_at: position(1, 3)?})) {
            return Err("old S5 fresh old-epoch renewal lost stale3/cutoff1:3".into());
        }
        original_rows(&mut old.socket, database).await?;
        println!("NORMAL_REDEPLOY_PRELUDE S5_unread_R1_replacement_fresh_R2_original_expiry_superseding3_and_fresh_stale passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let old_closed = old.close().await;
    let target_closed = target.close().await;
    proof?;
    old_closed?;
    target_closed?;
    Ok(())
}

pub(super) async fn same_socket_zero_window(
    base: &str,
    platform: &collaboration_central_tokio::TokioCentralPlatform,
    database: DatabaseId,
) -> ConformanceResult<()> {
    let mut old = NormalPeer::enroll(base, "old").await?;
    let proof = async {
        prepare::claim(&mut old.socket, database).await?;
        original_rows(&mut old.socket, database).await?;
        let expected = ReportSchemaSupport {peer: old.peer, session_id: old.session,
            attach_seq: AttachSequence::new(1)?, support: SchemaSupport {max_version: 1}};
        if platform.conformance_schema_report(old.peer).await? != Some(expected.clone()) {
            return Err("old S6 did not start on its original exact schema1 attachment".into());
        }
        // The retained local caller supplies Duration::ZERO: no hibernation or elapsed-time claim.
        println!("NORMAL_REDEPLOY_PRELUDE S6_zero_window_original_socket_before_second_frame");
        original_rows(&mut old.socket, database).await?;
        if platform.conformance_schema_report(old.peer).await? != Some(expected) {
            return Err("old S6 second frame used another Hello instead of its original socket".into());
        }
        println!("NORMAL_REDEPLOY_PRELUDE S6_zero_window_same_socket_second_exact_complete_imported_prefix passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let closed = old.close().await;
    proof?;
    closed?;
    Ok(())
}
