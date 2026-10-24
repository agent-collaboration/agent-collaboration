//! Invalid owner requests must not poison or publish the retained native seed.
#![allow(clippy::panic_in_result_fn)]
#[path = "upload_support/central_fixture.rs"]
mod central_fixture;
mod support;
#[path = "upload_support/mod.rs"]
#[allow(
    dead_code,
    reason = "The other publication helpers serve sibling importer tests."
)]
mod upload_support;

use board_legacy_upgrade::UpgradePublisher;
use collaboration_central_conformance::{ConformanceClient, JwksFixtureServer};
use collaboration_central_core::{
    AdminImportRefusal, AdminProjectImportAnswer, AdminProjectImportRequest,
};
use fleet_sync_protocol::{DatabaseId, PageDigest, RequestId, SeedPageAnswer, SnapshotPage};
use support::*;

async fn request(
    http: &reqwest::Client,
    route: &str,
    request: &AdminProjectImportRequest,
) -> TestResult<AdminProjectImportAnswer> {
    Ok(http
        .post(route)
        .json(request)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

#[tokio::test]
async fn owner_page_refusals_leave_staged_bytes_unchanged_and_the_valid_import_resumes()
-> TestResult {
    let prepared = prepare("legacy-complete.sqlite").await?;
    let header = upload_support::header(&prepared).await?;
    let database = header.database();
    let project = prepared.projects.first().ok_or("project missing")?;
    let mut snapshot = project.database.snapshot().await?;
    let contents = snapshot.page(None).await?;
    snapshot.close().await?;
    let digest = PageDigest::of(&contents)?;
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start()
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)?;
    let server = central_fixture::start_server(root.path(), &jwks).await?;
    let observed:TestResult=async {
        let http=central_fixture::owner_client()?;
        let route=format!("{}/admin/v1/import/projects/{database}",server.base_url());
        let begin=AdminProjectImportRequest::Begin {header:header.clone()};
        let no_owner=reqwest::Client::builder().no_proxy().build()?.post(&route).json(&begin).send().await?;
        assert_eq!(no_owner.status(),reqwest::StatusCode::UNAUTHORIZED);
        let client=ConformanceClient::new(server.base_url()).map_err(|error|error as Box<dyn std::error::Error>)?;
        client.import_fleet(&prepared.report.fleet).await.map_err(|error|error as Box<dyn std::error::Error>)?;
        let another=DatabaseId::new(uuid::Uuid::now_v7())?;
        let wrong_route=format!("{}/admin/v1/import/projects/{another}",server.base_url());
        assert_eq!(request(&http,&wrong_route,&begin).await?,AdminProjectImportAnswer::Refused(AdminImportRefusal::IdentityMismatch));
        let page=|index,import_id,contents:SnapshotPage,page_digest|AdminProjectImportRequest::Page {
            digest:header.digest.clone(),import_id,page:index,contents,page_digest,
        };
        assert_eq!(request(&http,&route,&page(0,RequestId::new(uuid::Uuid::now_v7())?,contents.clone(),digest.clone())).await?,
            AdminProjectImportAnswer::Refused(AdminImportRefusal::NotFound));
        let copy=format!("copy-{database}");
        assert!(!root.path().join(format!("{copy}.db")).exists());
        let begun=request(&http,&route,&begin).await?;
        let AdminProjectImportAnswer::Ready {import_id,..}=begun else {return Err("valid Begin did not reserve its actual seed".into());};
        assert_eq!(request(&http,&route,&AdminProjectImportRequest::Complete {digest:header.digest.clone(),import_id}).await?,
            AdminProjectImportAnswer::Refused(AdminImportRefusal::NotPublished));
        let staged="SELECT payload AS value FROM _fleet_sync_seed_pages ORDER BY page";
        assert!(upload_support::read(root.path(),&copy,staged).await?.is_empty());

        let mut wrong_database=contents.clone();
        wrong_database.database=another;
        for (offered,expected) in [
            (page(header.pages,import_id,contents.clone(),digest.clone()),AdminImportRefusal::InvalidHeader),
            (page(0,RequestId::new(uuid::Uuid::now_v7())?,contents.clone(),digest.clone()),AdminImportRefusal::IdentityMismatch),
            (page(0,import_id,wrong_database,digest.clone()),AdminImportRefusal::InvalidHeader),
            (page(0,import_id,contents.clone(),PageDigest::new("0".repeat(64))?),AdminImportRefusal::PageRefused(SeedPageAnswer::SeedConflict)),
        ] {
            assert_eq!(request(&http,&route,&offered).await?,AdminProjectImportAnswer::Refused(expected));
            assert!(upload_support::read(root.path(),&copy,staged).await?.is_empty());
        }
        let valid=page(0,import_id,contents.clone(),digest.clone());
        let expected=AdminProjectImportAnswer::Staged {import_id,page:0};
        assert_eq!(request(&http,&route,&valid).await?,expected);
        let before=upload_support::read(root.path(),&copy,staged).await?;
        assert_eq!(before.len(),1);
        assert_eq!(request(&http,&route,&valid).await?,expected);
        assert_eq!(upload_support::read(root.path(),&copy,staged).await?,before);
        let mut changed=contents.clone();
        changed.rows.clear();
        let changed_digest=PageDigest::of(&changed)?;
        assert_ne!(changed_digest,digest);
        assert_eq!(request(&http,&route,&page(0,import_id,changed,changed_digest)).await?,
            AdminProjectImportAnswer::Refused(AdminImportRefusal::PageRefused(SeedPageAnswer::SeedConflict)));
        assert_eq!(upload_support::read(root.path(),&copy,staged).await?,before);
        assert_eq!(upload_support::read(root.path(),&copy,
            "SELECT CAST(count(*) AS TEXT) AS value FROM sqlite_schema WHERE type='table' AND name='posts'").await?,vec!["0"]);
        assert_eq!(upload_support::read(root.path(),&copy,
            "SELECT CAST(count(*) AS TEXT) AS value FROM _copy_summary").await?,vec!["0"]);

        // Positive control: neither bad input nor duplicate/conflicting pages
        // prevents the genuine publisher from finishing this same seed.
        let publisher=UpgradePublisher::new(reqwest::Url::parse(server.base_url())?,http);
        let completed=publisher.publish(&prepared).await?;
        assert!(completed.projects.iter().all(|project|matches!(project.answer,AdminProjectImportAnswer::Imported {..})));
        upload_support::published_copies(root.path(),&prepared,None).await?;
        Ok(())
    }.await;
    prepared.close().await;
    let stopped = server.shutdown().await;
    let keys_stopped = jwks.shutdown().await;
    stopped?;
    keys_stopped.map_err(|error| error as Box<dyn std::error::Error>)?;
    observed
}
