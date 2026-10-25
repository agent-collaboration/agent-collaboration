//! Refusals must cross the normal owner route from real Registry/Copy state.
use super::*;
use collaboration_central_conformance::{DispatchFaultObservation, RuntimeControl};

#[tokio::test]
async fn owner_takeover_preserves_schema_too_new_from_the_real_target_hello()
-> ConformanceResult<()> {
    let mut fixture = Fixture::open().await?;
    let proof = async {
        fixture.target_schema(0).await?;
        let piece = Piece::<board_sync_protocol::BoardAnnotation>::new(PieceContents {
            database: fixture.database, epoch: Epoch::new(1)?,
            continues_from: fixture::position(1,0)?, through: fixture::position(1,1)?,
            transactions: vec![ReplicatedTransaction::SchemaStep {
                commit_id: CommitSequence::new(1), migration_version:1,
                statements: vec!["CREATE TABLE schema_target_rows(id TEXT NOT NULL PRIMARY KEY)".into()],
            }],part:None,annotation:None,
        })?;
        if !matches!(support::answer(&mut fixture.old, OperationRequest::Replication(ReplicationRequest::Piece(piece))).await?,
            OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through == fixture::position(1,1)?) {
            return Err("schema refusal setup did not commit schema1 through the old holder".into());
        }
        let status = support::answer(&mut fixture.old, OperationRequest::Replication(ReplicationRequest::CopyStatus { database:fixture.database })).await?;
        let OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) = status else {
            return Err("schema refusal setup has no real Copy schema".into());
        };
        if status.schema.version != 1 { return Err("schema refusal setup did not reach version1".into()); }
        let preview = fixture.preview().await?;
        let reply = fixture.post("takeover", &confirm(durable_steps::Uuid::now_v7(),preview)).await?;
        if reply != json!({"kind":"refused","value":{"kind":"schemaTooNew","value":{"store":status.schema,"supported":0}}}) {
            return Err(format!("owner takeover erased the actual schema refusal: {reply}").into());
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let database = fixture.database;
    let root = fixture.close().await?;
    proof?;
    let grant = fixture::snapshot(root.path(), database)
        .await?
        .grant
        .ok_or("schema refusal lost the old grant")?;
    if grant.epoch != Epoch::new(1)? {
        return Err("schema refusal granted a new epoch".into());
    }
    Ok(())
}

#[tokio::test]
async fn owner_takeover_preserves_transfer_in_progress_while_the_first_real_fence_is_held()
-> ConformanceResult<()> {
    let fixture = Fixture::open().await?;
    let mut pending = tokio::task::JoinSet::new();
    let proof=async {
        let preview=fixture.preview().await?;
        let first=durable_steps::Uuid::now_v7();
        fixture.control(RuntimeControl::HoldNextDispatch { destination: collaboration_central_core::ObjectAddress::Copy(fixture.database) }).await?;
        let http=fixture.http.clone(); let route=fixture.route("takeover"); let first_request=confirm(first,preview.clone()); let request=first_request.clone();
        pending.spawn(async move { fixture::post(&http,&route,&request).await });
        let observed=fixture.control(RuntimeControl::AwaitHeldDispatch).await?;
        let DispatchFaultObservation::Held {destination,request}=observed.dispatch else { return Err("first transfer did not reach its real Copy fence hold".into()); };
        if destination != collaboration_central_core::ObjectAddress::Copy(fixture.database) { return Err("transfer held another Copy".into()); }
        let held:Value=serde_json::from_slice(&request)?;
        if held.get("kind") != Some(&json!("fleet-sync.copy-fence")) { return Err(format!("transfer hold caught another operation: {held}").into()); }
        let reply=fixture.post("takeover",&confirm(durable_steps::Uuid::now_v7(),preview)).await?;
        if reply != json!({"kind":"refused","value":{"kind":"transferInProgress","value":{"transfer":first}}}) {
            return Err(format!("second owner transfer lost the first active attempt: {reply}").into());
        }
        fixture.control(RuntimeControl::ResumeDispatch).await?;
        let answer=tokio::time::timeout(Duration::from_secs(8),pending.join_next()).await?.ok_or("first transfer vanished")???;
        fixture::granted(&fixture.complete("takeover",&first_request,answer).await?,"granted",2,fixture::position(1,0)?)?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    // Always release our exact owned hold before shutting down or reporting a failed assertion.
    let _ = fixture.control(RuntimeControl::ResumeDispatch).await;
    pending.abort_all();
    while pending.join_next().await.is_some() {}
    let database = fixture.database;
    let root = fixture.close().await?;
    proof?;
    let grant = fixture::snapshot(root.path(), database)
        .await?
        .grant
        .ok_or("first transfer did not persist its grant")?;
    if grant.epoch != Epoch::new(2)? {
        return Err("concurrent refusal changed the winning grant".into());
    }
    Ok(())
}

#[tokio::test]
async fn owner_takeover_reports_registry_epoch_unproven_at_the_actual_epoch_ceiling()
-> ConformanceResult<()> {
    let fixture = Fixture::open().await?;
    let proof=async {
        // Only arrangement uses the existing conformance ingress. It invokes the
        // real CopyFence; the claimed refusal goes through normal owner HTTP.
        let request=FenceRequest {
            database:fixture.database,transfer_id:TransferId::new(durable_steps::Uuid::now_v7())?,
            new_epoch:Epoch::new(u64::MAX)?,
            expect:FenceExpectation {epoch:Epoch::new(1)?,acked:fixture::position(1,0)?},
            target_schema:Some(SchemaSupport {max_version:1}),holder:PeerId::try_from(PEER.to_owned())?,
        };
        let copy=configuration::registry_config()?.authoritative_receiver;
        let response=fixture.copy_probe(&json!({"attempt":durable_steps::Uuid::now_v7(),"copy":copy,"request":{"kind":"fence","value":request}})).await?;
        let fenced: FenceResult=serde_json::from_value(response.pointer("/value/value").cloned().ok_or_else(||format!("epoch-ceiling setup did not complete the actual fence: {response}"))?)?;
        if !matches!(fenced,FenceResult::Fenced {new_epoch,..} if new_epoch==Epoch::new(u64::MAX)?) { return Err("epoch-ceiling setup did not commit the exact ceiling fence".into()); }
        let preview=fixture.preview().await?;
        if preview.get("fence") != Some(&json!(u64::MAX)) { return Err("epoch-ceiling preview did not read the real Copy fence".into()); }
        refused(&fixture.post("takeover",&confirm(durable_steps::Uuid::now_v7(),preview)).await?,"registryEpochUnproven")?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let database = fixture.database;
    let root = fixture.close().await?;
    proof?;
    let grant = fixture::snapshot(root.path(), database)
        .await?
        .grant
        .ok_or("ceiling refusal lost original Registry grant")?;
    if grant.epoch != Epoch::new(1)? {
        return Err("ceiling refusal nevertheless granted a new epoch".into());
    }
    Ok(())
}

#[tokio::test]
async fn owner_preview_reports_absence_and_rejects_machine_authority_before_the_task()
-> ConformanceResult<()> {
    let fixture = Fixture::open().await?;
    let proof = async {
        let absent = DatabaseId::new(durable_steps::Uuid::now_v7())?;
        let route = fixture
            .route("takeover/preview")
            .replace(&fixture.database.to_string(), &absent.to_string());
        if fixture::post(&fixture.http, &route, &Value::Null).await? != json!({"kind":"notFound"}) {
            return Err("owner preview fabricated a missing Registry project".into());
        }
        let response = fixture
            .http
            .post(fixture.route("takeover/preview"))
            .header(
                "Cf-Access-Jwt-Assertion",
                collaboration_central_conformance::machine_assertion("directory writer")?,
            )
            .bearer_auth("directory writer")
            .send()
            .await?;
        if response.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Err("machine credential acquired owner preview authority".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.close().await?;
    proof
}
