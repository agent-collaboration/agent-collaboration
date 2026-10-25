//! Observe automatic convergence before replaying the original Claim or archive.
use super::*;
use fleet_sync_protocol::{LeaseAnswer, LeaseRequest};

pub(super) async fn verify_cleanup(
    base: &str,
    root: &std::path::Path,
    input: &CleanupInput,
    journal: &SavedJournal,
    loss: CleanupLoss,
) -> ConformanceResult<()> {
    observe_exact_recovered_call(base, input).await?;
    let mut reader = registration::reconnect_peer(base, READER_CREDENTIAL, 1).await?;
    let proof=async {
        let expected=match &input.settled_before {Some(before)=>before.clone(),None=>finalized(&input.before,RegistryTime(input.at.0+chrono::TimeDelta::seconds(6)))?};
        tokio::time::timeout(RECOVERY_BOUND,async {
            loop {
                if read_fleet(&mut reader).await?==expected {return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());}
                tokio::task::yield_now().await;
            }
        }).await.map_err(|_|"startup did not apply the original refused settlement and exact archive finalization")??;
        // This is the existing durable-journal observation used by the physical
        // recovery cases. No Claim replay is allowed to drive this completion.
        let path=root.join("registry.db");
        tokio::time::timeout(RECOVERY_BOUND,async {
            loop {
                if operation(&path,&input.operation).await?.0=="completed" {return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());}
                tokio::task::yield_now().await;
            }
        }).await.map_err(|_|"original refused-settlement debt kept retrying after recipient finalization")??;
        if lease_publication::reservation(base,input.database,match &input.original.change {HolderChangeRequest::Claim {attempt,..}=>*attempt,_=>return Err("saved settlement is not Claim".into())}).await?
            !=Some(ClaimReservationState::Settled {outcome:ClaimReservationOutcome::Refused}) {return Err("recovered settlement did not retain Refused for its exact original attempt".into());}
        let before_ledger=exact_ledger_decision(base,input.archive.request_id.clone()).await?;
        if decide(&mut reader,&input.archive).await?!=input.decision {return Err("archive replay changed its exact original request decision".into());}
        if exact_ledger_decision(base,input.archive.request_id.clone()).await?!=before_ledger {return Err("archive replay changed the original ledger entry".into());}
        let mut claimant=registration::reconnect_peer(base,CREDENTIAL,0).await?;
        let claim_proof=async {
            let HolderChangeRequest::Claim {attempt,database,..}=input.original.change else {return Err("saved cleanup changed request kind".into());};
            let answer=creation_scenarios::operation_answer(&mut claimant,OperationRequest::Lease(LeaseRequest::Claim {attempt:fleet_sync_protocol::RequestId::new(attempt.as_uuid())?,database})).await?;
            let fleet_sync_protocol::SettleOutcome::Refused {reason}=&input.original.outcome else {return Err("saved cleanup lost its refusal".into());};
            if !matches!(answer,OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Refused(actual))) if actual==*reason) {return Err("normal Claim replay lost its original terminal schema refusal".into());}
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        let claimant_closed=claimant.close(reqwest_websocket::CloseCode::Normal,None).await;
        claim_proof?;claimant_closed?;
        let terminal=operation(&path,&input.operation).await?;
        let later=RegistryTime(chrono::Utc::now()+chrono::TimeDelta::seconds(60));
        for address in [ObjectAddress::Registry,ObjectAddress::Fleet] {control(base,address,RuntimeControl::SetTime {now:later}).await?;}
        if read_fleet(&mut reader).await?!=expected || exact_ledger_decision(base,input.archive.request_id.clone()).await?!=before_ledger {
            return Err("later wake repeated archive records/feed or changed the stored request ledger".into());
        }
        if operation(&path,&input.operation).await?!=terminal {return Err("replay or later wake repeated original settlement debt".into());}
        verify_one_terminal_input(root,&input.operation,journal).await?;
        let saved:ClaimSettlement=serde_json::from_value(input.call.get("request").cloned().ok_or("saved call lost payload")?)?;
        if saved!=input.original {return Err("cleanup changed its exact original settlement payload".into());}
        println!("ARCHIVE_CLEANUP_RESTART_STAGE loss={} exact_public_finalization_and_terminal_original_debt",match loss {CleanupLoss::Dispatch=>"dispatch",CleanupLoss::Acknowledgement=>"acknowledgement"});
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = reader
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    proof?;
    closed?;
    Ok(())
}

async fn observe_exact_recovered_call(base: &str, input: &CleanupInput) -> ConformanceResult<()> {
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(RECOVERY_BOUND + Duration::from_secs(2))
        .build()?;
    tokio::time::timeout(RECOVERY_BOUND,async {
        loop {
            let observed=http.post(format!("{base}/admin/v1/conformance/runtime"))
                .header("Cf-Access-Jwt-Assertion",owner_assertion()?)
                .json(&RuntimeControlRequest {address:ObjectAddress::Registry,command:RuntimeControl::AwaitHeldDispatch})
                .send().await?.error_for_status()?.json::<collaboration_central_conformance::RuntimeFaultObservation>().await?;
            let DispatchFaultObservation::Held {destination,request}=observed.dispatch else {return Err("startup retry never reached its pre-armed real transport gate".into());};
            if destination!=ObjectAddress::Fleet {return Err("startup retry reached another destination".into());}
            let call:Value=serde_json::from_slice(&request)?;
            if call.get("kind")==Some(&json!(<fleet_sync_lease::ClaimSettlement as durable_steps::CallTarget>::KIND)) {
                if call!=input.call {return Err(format!("recovered settlement changed its exact original call identity or payload: {call}").into());}
                control(base,ObjectAddress::Registry,RuntimeControl::ResumeDispatch).await?;
                println!("ARCHIVE_CLEANUP_RESTART_STAGE actual_retried_call_identity_and_payload_unchanged");
                return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());
            }
            // A fresh run may allocate/publish its ordinary metadata before
            // the killed drive's existing lease expires. Those calls stay real.
            if !matches!(call.get("kind").and_then(Value::as_str),Some("central.lease-incarnation"|"central.lease-projection")) {
                return Err(format!("startup recovery intercepted an unexpected Fleet call: {call}").into());
            }
            control(base,ObjectAddress::Registry,RuntimeControl::ResumeDispatchAndHoldNext {destination:ObjectAddress::Fleet}).await?;
        }
    }).await.map_err(|_|"startup did not reconstruct the exact original refused-settlement call")?
}

async fn verify_one_terminal_input(
    root: &std::path::Path,
    operation: &Value,
    journal: &SavedJournal,
) -> ConformanceResult<()> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("registry.db"))
        .read_only(true)
        .connect()
        .await?;
    let result=async {
        let row=query::<sqlx_turso::Turso>("SELECT input_digest,(SELECT COUNT(*) FROM _fleet_sync_operations WHERE kind=? AND input_digest=?) AS copies,(SELECT COUNT(*) FROM _fleet_sync_operations WHERE kind=? AND input_digest=? AND state!='completed') AS pending FROM _fleet_sync_operations WHERE operation_id=?")
            .bind(journal.kind.clone()).bind(journal.digest.clone()).bind(journal.kind.clone()).bind(journal.digest.clone()).bind(operation.as_str().ok_or("original operation is not text")?)
            .fetch_one(&mut connection).await?;
        if row.try_get::<Vec<u8>,_>("input_digest")?!=journal.digest || row.try_get::<i64,_>("copies")?!=1 ||row.try_get::<i64,_>("pending")?!=0 {return Err("original input digest, one operation or terminal settlement debt changed".into());}
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed = connection.close().await;
    result?;
    closed?;
    Ok(())
}
