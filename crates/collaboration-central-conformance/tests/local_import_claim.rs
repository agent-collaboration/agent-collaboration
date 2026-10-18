#![allow(clippy::expect_used)]
use collaboration_central_conformance::*;
use collaboration_central_core::*;
use collaboration_central_local::{LocalServer, LocalSettings};
#[tokio::test]
async fn fleet_conflict_preserves_exact_decision_records_and_feed() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = collaboration_central_conformance::ConformanceClient::new(server.base_url())
        .expect("client");
    let result = collaboration_central_conformance::run_fleet_conflict_conformance(&client).await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    result.expect("Fleet conflicting request real route");
}
#[tokio::test]
async fn ordinary_fleet_admission_and_first_epoch_run_through_real_session() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = collaboration_central_conformance::ConformanceClient::new(server.base_url())
        .expect("client");
    let result = collaboration_central_conformance::run_fleet_request_conformance(&client).await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    result.expect("actual Fleet requests/creation chain");
}
#[tokio::test]
async fn fleet_import_owner_route_is_atomic_and_preserves_original_records() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = collaboration_central_conformance::ConformanceClient::new(server.base_url())
        .expect("client");
    let result = collaboration_central_conformance::run_fleet_import_conformance(&client).await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    result.expect("actual Fleet owner import and exact reads");
}
use collaboration_replication_protocol::*;
#[tokio::test]
async fn owner_import_publishes_an_unheld_readable_copy() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let proof=async {
        let client=ConformanceClient::new(server.base_url())?;
        let project=ProjectId::new(uuid::Uuid::now_v7().to_string())?;
        let head=ReplicationPosition::new(Epoch::new(1)?,0.into());
        let header=ImportSnapshotHeader{project:project.clone(),head,schema:SchemaIdentity::new(0,schema_fingerprint(&[])?)?,schema_steps:Vec::new()};
        let summary=ProjectSummary{status:"open".into(),latest_record_at:chrono::Utc::now()};
        let upload=ProjectImportPage{digest:ImportDigestBuilder::new(&header)?.finish(&summary)?,index:0,page:SnapshotPage{project:project.clone(),position:head,schema:header.schema,schema_steps:header.schema_steps,table:String::new(),rows:Vec::new(),next:None,as_of:chrono::Utc::now()},final_summary:Some(summary)};
        assert_eq!(client.import_project(&project,&upload).await?,ProjectImportAnswer::Imported{head});
        let result=client.call(collaboration_central_core::ObjectAddress::Copy(project.clone()),collaboration_central_core::WireCall::Copy(CopyCall::CopyStatus{project})).await?;
        assert!(matches!(result,collaboration_central_core::WireResult::Copy(CopyResult::CopyStatus(Some(state))) if state.holder.is_none() && state.acked==head));
        Ok::<(),Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    proof.expect("owner import");
}

#[tokio::test]
async fn import_claim_race_and_validation_use_real_turso_and_websockets() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    let proof = run_import_claim_conformance(&client).await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    proof.expect("import conformance");
}

#[tokio::test]
async fn import_claim_and_directory_producer_resume_after_native_server_restart() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let server = LocalServer::start(settings()).await.expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    let prepared = async {
        let directory = ImportDirectoryRestartScenario::prepare(&client).await?;
        let claim = ImportClaimRestartScenario::prepare(&client).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((directory, claim))
    }
    .await;
    server.shutdown().await.expect("shutdown");
    let (directory, claim) = prepared.expect("prepared");
    let server = LocalServer::start(settings()).await.expect("restarted");
    let client = ConformanceClient::new(server.base_url()).expect("new client");
    let verified = async {
        claim.verify(&client).await?;
        directory.verify(&client).await
    }
    .await;
    server.shutdown().await.expect("restart shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    verified.expect("restart proof");
}
