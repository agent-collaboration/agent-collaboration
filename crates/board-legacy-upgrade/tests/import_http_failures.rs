//! Actual owner import status/body failures preserve retained workflow identity.
#![allow(clippy::panic_in_result_fn)]
#[path = "upload_support/central_fixture.rs"]
mod central_fixture;
mod support;
#[path = "upload_support/mod.rs"]
#[allow(
    dead_code,
    reason = "Sibling publication tests use the remaining helpers."
)]
mod upload_support;

use board_legacy_upgrade::{PreparedUpgrade, UpgradePublisher, UpgradeUploadError};
use collaboration_central_admin_protocol::OwnerOperationFailure;
use collaboration_central_conformance::{JwksFixtureServer, RuntimeControl, RuntimeControlRequest};
use collaboration_central_core::{
    AdminImportHeader, AdminProjectImportAnswer, AdminProjectImportRequest, ObjectAddress,
};
use collaboration_central_tokio::TokioServer;
use fleet_sync_protocol::{PageDigest, RequestId, SeedId};
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::{num::NonZeroU64, time::Duration};
use support::*;

async fn request(
    http: &reqwest::Client,
    base: &str,
    header: &AdminImportHeader,
    input: &AdminProjectImportRequest,
) -> TestResult<(StatusCode, Value)> {
    let response = http
        .post(format!(
            "{base}/admin/v1/import/projects/{}",
            header.database()
        ))
        .json(input)
        .send()
        .await?;
    Ok((response.status(), response.json().await?))
}

async fn begin(
    server: &TokioServer,
    prepared: &PreparedUpgrade,
) -> TestResult<(AdminImportHeader, RequestId, SeedId)> {
    let http = central_fixture::owner_client()?;
    http.post(format!("{}/admin/v1/import/fleet", server.base_url()))
        .json(&prepared.report.fleet)
        .send()
        .await?
        .error_for_status()?;
    let header = upload_support::header(prepared).await?;
    let input = AdminProjectImportRequest::Begin {
        header: header.clone(),
    };
    let (import_id, seed) = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (status, body) = request(&http, server.base_url(), &header, &input).await?;
            assert!(status.is_success(), "{status}: {body}");
            match serde_json::from_value(body)? {
                AdminProjectImportAnswer::Ready { import_id, seed } => {
                    return Ok::<_, Box<dyn std::error::Error>>((import_id, seed));
                }
                AdminProjectImportAnswer::Pending => tokio::task::yield_now().await,
                other => return Err(format!("unexpected reservation answer: {other:?}").into()),
            }
        }
    })
    .await??;
    Ok((header, import_id, seed))
}

async fn arm_full(server: &TokioServer, header: &AdminImportHeader) -> TestResult {
    central_fixture::owner_client()?
        .post(format!(
            "{}/admin/v1/conformance/runtime",
            server.base_url()
        ))
        .json(&RuntimeControlRequest {
            address: ObjectAddress::Copy(header.database()),
            command: RuntimeControl::CopyFull {
                after_writes: NonZeroU64::new(1).ok_or("positive threshold")?,
            },
        })
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

#[tokio::test]
async fn retired_import_returns_gone_with_the_owner_body() -> TestResult {
    let (_source_directory, source) = fixture_copy("legacy-complete.sqlite")?;
    let legacy = rusqlite::Connection::open(&source)?;
    legacy.execute("UPDATE project_boards SET state='archived'", [])?;
    legacy.close().map_err(|(_, error)| error)?;
    let prepared = prepare_path(&source).await?;
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let observed:TestResult = async {
        let http=central_fixture::owner_client()?;
        UpgradePublisher::new(server.base_url().parse()?,http.clone()).publish(&prepared).await?;
        let header=upload_support::header(&prepared).await?;
        let retirement=json!({"requestId":uuid::Uuid::now_v7(),"expectFence":1,"expectAcked":header.manifest.position});
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let answer:Value=http.post(format!("{}/admin/v1/projects/{}/retire",server.base_url(),header.database())).json(&retirement).send().await?.error_for_status()?.json().await?;
                if answer["kind"]=="retired" {return Ok::<_,Box<dyn std::error::Error>>(());}
                assert_eq!(answer["kind"],"pending", "{answer}");
                tokio::task::yield_now().await;
            }
        }).await??;
        // Act/assert: replay the real completed import after its Copy was retired.
        let (status,body)=request(&http,server.base_url(),&header,&AdminProjectImportRequest::Begin {header:header.clone()}).await?;
        assert_eq!(status,StatusCode::GONE,"{body}");
        assert_eq!(body,json!({"kind":"retired"}));
        let error=UpgradePublisher::new(server.base_url().parse()?,http).publish(&prepared).await.unwrap_err();
        assert!(matches!(error,UpgradeUploadError::OwnerFailure {failure:OwnerOperationFailure::Retired}));
        Ok(())
    }.await;
    server.shutdown().await?;
    jwks.shutdown()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    prepared.close().await;
    observed
}

#[tokio::test]
async fn full_copy_import_returns_typed_503_and_keeps_the_same_seed() -> TestResult {
    let prepared = prepare("legacy-complete.sqlite").await?;
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let observed:TestResult=async {
        let (header,import_id,seed)=begin(&server,&prepared).await?;
        let mut snapshot=prepared.projects.first().ok_or("project missing")?.database.snapshot().await?;
        let contents=snapshot.page(None).await?;
        snapshot.close().await?;
        let page=AdminProjectImportRequest::Page {digest:header.digest.clone(),import_id,page:0,page_digest:PageDigest::of(&contents)?,contents};
        arm_full(&server,&header).await?;
        // Act/assert: a real seed write rolls back at the existing storage fault boundary.
        let (status,body)=request(&central_fixture::owner_client()?,server.base_url(),&header,&page).await?;
        assert_eq!(status,StatusCode::SERVICE_UNAVAILABLE,"{body}");
        assert_eq!(body,json!({"kind":"centralCopyFull"}));
        let (_,same_import,same_seed)=begin(&server,&prepared).await?;
        assert_eq!((same_import,same_seed),(import_id,seed));
        arm_full(&server,&header).await?;
        let error=UpgradePublisher::new(server.base_url().parse()?,central_fixture::owner_client()?).publish(&prepared).await.unwrap_err();
        assert!(matches!(error,UpgradeUploadError::OwnerFailure {failure:OwnerOperationFailure::CentralCopyFull}));
        // Recovery still publishes through the real owner workflow, without rekeying.
        UpgradePublisher::new(server.base_url().parse()?,central_fixture::owner_client()?).publish(&prepared).await?;
        let (_,status)=request(&central_fixture::owner_client()?,server.base_url(),&header,&AdminProjectImportRequest::Status {digest:header.digest.clone()}).await?;
        assert!(matches!(serde_json::from_value(status)?,AdminProjectImportAnswer::Progress(record) if record.import_id==import_id));
        Ok(())
    }.await;
    server.shutdown().await?;
    jwks.shutdown()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    prepared.close().await;
    observed
}

#[tokio::test]
async fn pending_copy_import_keeps_the_owner_reason_across_reopen() -> TestResult {
    let prepared = prepare("legacy-complete.sqlite").await?;
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let (header, import_id, seed) = begin(&server, &prepared).await?;
    server.shutdown().await?;
    let object = format!("copy-{}", header.database());
    let saved=upload_support::read(root.path(),&object,"SELECT json_array(row_id,corpus,version,checksum) AS value FROM central_schema_migrations WHERE corpus='fleet-sync-receiver' AND version='3'").await?;
    let [saved] = saved.as_slice() else {
        return Err("one receiver history stamp required".into());
    };
    let stamp: Vec<String> = serde_json::from_str(saved)?;
    upload_support::execute(
        root.path(),
        &object,
        "DELETE FROM central_schema_migrations WHERE corpus='fleet-sync-receiver' AND version='3'",
    )
    .await?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let observed:TestResult=async {
        // Act/assert: the actual reopened Copy prelude supplies its shared typed reason.
        let (status,body)=request(&central_fixture::owner_client()?,server.base_url(),&header,&AdminProjectImportRequest::Begin {header:header.clone()}).await?;
        assert_eq!(status,StatusCode::SERVICE_UNAVAILABLE,"{body}");
        assert_eq!(body,json!({"kind":"copyUnavailable","reason":{"kind":"migrationPending","value":{"version":3}}}));
        let error=UpgradePublisher::new(server.base_url().parse()?,central_fixture::owner_client()?).publish(&prepared).await.unwrap_err();
        assert!(matches!(error,UpgradeUploadError::OwnerFailure {failure:OwnerOperationFailure::CopyUnavailable {reason:fleet_sync_protocol::CopyUnavailableReason::MigrationPending {version:3}}}));
        Ok(())
    }.await;
    server.shutdown().await?;
    use sqlx_turso::sqlx::{ConnectOptions, Connection};
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.path().join(format!("{object}.db")))
        .connect()
        .await?;
    let restored = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(
        "INSERT INTO central_schema_migrations(row_id,corpus,version,checksum) VALUES(?,?,?,?)",
    )
    .bind(&stamp[0])
    .bind(&stamp[1])
    .bind(&stamp[2])
    .bind(&stamp[3])
    .execute(&mut connection)
    .await;
    connection.close().await?;
    restored?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let recovery: TestResult = async {
        let (_, same_import, same_seed) = begin(&server, &prepared).await?;
        assert_eq!((same_import, same_seed), (import_id, seed));
        UpgradePublisher::new(server.base_url().parse()?, central_fixture::owner_client()?)
            .publish(&prepared)
            .await?;
        Ok(())
    }
    .await;
    server.shutdown().await?;
    jwks.shutdown()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    prepared.close().await;
    observed?;
    recovery
}
