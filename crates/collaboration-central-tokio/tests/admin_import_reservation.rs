//! Real owner HTTP, Fleet continuation, Registry ImportTask and receiver staging.
#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]

use collaboration_central_conformance::{
    ConformanceResult, JwksFixtureServer, fixture_policy, owner_assertion,
};
use collaboration_central_core::{
    AdminImportHeader, AdminImportPhase, AdminImportRefusal, AdminProjectImportAnswer,
    AdminProjectImportRequest,
};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use fleet_sync_protocol::{PageDigest, SeedId, SnapshotPage};
use sqlx_core::row::Row;
use sqlx_turso::sqlx::{ConnectOptions, Connection};
use std::{path::Path, time::Duration};

#[path = "native_http/support.rs"]
mod configuration;

const PROJECT: &str = "019f0000-0000-7000-8000-000000000601";

fn header() -> ConformanceResult<AdminImportHeader> {
    Ok(serde_json::from_value(serde_json::json!({
        "digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "provenance":{"installation":"550e8400-e29b-41d4-a716-446655440000","machine":"original import machine"},
        "declaredOwner":"first-importer",
        "manifest":{
            "database":PROJECT,"position":{"epoch":1,"sequence":1},
            "schema":{"version":1,"fingerprint":"staging-fixture"},"tables":[]
        },
        "pages":1,
        "summary":{"kind":"projectSummary","value":{"status":null,"latestRecordAt":{"kind":"legacyUnknown"}}}
    }))?)
}

async fn start(root: &Path, jwks: &JwksFixtureServer) -> ConformanceResult<TokioServer> {
    Ok(TokioServer::start(TokioSettings {
        delivery: collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        root: root.into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?)
}

async fn request(
    server: &TokioServer,
    input: &AdminProjectImportRequest,
) -> ConformanceResult<AdminProjectImportAnswer> {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!(
            "{}/admin/v1/import/projects/{PROJECT}",
            server.base_url()
        ))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(input)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(format!("project import status {status}: {body}").into());
    }
    Ok(serde_json::from_str(&body)?)
}

async fn begin(
    server: &TokioServer,
    header: &AdminImportHeader,
) -> ConformanceResult<(fleet_sync_protocol::RequestId, SeedId)> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match request(
                server,
                &AdminProjectImportRequest::Begin {
                    header: header.clone(),
                },
            )
            .await?
            {
                AdminProjectImportAnswer::Ready { import_id, seed } => {
                    return Ok((import_id, seed));
                }
                AdminProjectImportAnswer::Pending => tokio::task::yield_now().await,
                other => return Err(format!("reservation expected Ready: {other:?}").into()),
            }
        }
    })
    .await?
}

async fn durable_rows(
    root: &Path,
    object: &str,
    statement: &'static str,
) -> ConformanceResult<Vec<String>> {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("{object}.db")))
        .read_only(true)
        .connect()
        .await?;
    let rows = sqlx_core::query::query::<sqlx_turso::Turso>(statement)
        .fetch_all(&mut connection)
        .await?;
    let result = rows
        .iter()
        .map(|row| row.try_get("snapshot"))
        .collect::<Result<Vec<String>, _>>()?;
    connection.close().await?;
    Ok(result)
}

#[tokio::test]
async fn reservation_and_seed_identity_survive_reopen_without_reporting_completion()
-> ConformanceResult<()> {
    let directory = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = start(directory.path(), &jwks).await?;
    let offered = header()?;
    let first = async {
        let identity = begin(&server, &offered).await?;
        assert_eq!(begin(&server, &offered).await?, identity);
        assert_eq!(
            request(
                &server,
                &AdminProjectImportRequest::Complete {
                    digest: offered.digest.clone(),
                    import_id: identity.0,
                }
            )
            .await?,
            AdminProjectImportAnswer::Refused(AdminImportRefusal::NotPublished)
        );
        let AdminProjectImportAnswer::Progress(record) = request(
            &server,
            &AdminProjectImportRequest::Status {
                digest: offered.digest.clone(),
            },
        )
        .await?
        else {
            return Err("stored reserved workflow was not readable".into());
        };
        assert_eq!(record.import_id, identity.0);
        assert_eq!(record.header, offered);
        assert_eq!(record.phase, AdminImportPhase::Reserved);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(identity)
    }
    .await;
    server.shutdown().await?;
    let identity = match first {
        Ok(value) => value,
        Err(error) => {
            jwks.shutdown().await?;
            return Err(error);
        }
    };
    let reopened = start(directory.path(), &jwks).await?;
    let observed = async {
        assert_eq!(begin(&reopened, &offered).await?, identity);
        let contents: SnapshotPage = serde_json::from_value(serde_json::json!({
            "database":PROJECT,"position":{"epoch":1,"sequence":1},
            "schema":{"version":1,"fingerprint":"staging-fixture"},
            "schemaSteps":[],"table":"staging_rows","rows":[],"next":null
        }))?;
        let page = AdminProjectImportRequest::Page {
            digest: offered.digest.clone(),
            import_id: identity.0,
            page: 0,
            page_digest: PageDigest::of(&contents)?,
            contents,
        };
        let staged = AdminProjectImportAnswer::Staged {
            import_id: identity.0,
            page: 0,
        };
        assert_eq!(request(&reopened, &page).await?, staged);
        assert_eq!(request(&reopened, &page).await?, staged);
        let page_sql = "SELECT json_object('seed',seed,'page',page,'digest',digest,'payload',payload) AS snapshot FROM _fleet_sync_seed_pages ORDER BY page";
        let workflow_sql = "SELECT json_object('importId',import_id,'project',project_id,'digest',digest,'header',header,'phase',phase) AS snapshot FROM admin_imports ORDER BY import_id";
        let copy = format!("copy-{PROJECT}");
        let pages_before = durable_rows(directory.path(), &copy, page_sql).await?;
        let workflow_before = durable_rows(directory.path(), "fleet", workflow_sql).await?;
        assert_eq!(pages_before.len(), 1);
        assert_eq!(workflow_before.len(), 1);
        assert_eq!(
            request(
                &reopened,
                &AdminProjectImportRequest::Complete {
                    digest: offered.digest.clone(),
                    import_id: identity.0,
                }
            )
            .await?,
            AdminProjectImportAnswer::Refused(AdminImportRefusal::SeedRefused(
                fleet_sync_protocol::SeedAnswer::SchemaMismatch
            ))
        );
        let mut changed = offered.clone();
        changed.pages = 2;
        changed.provenance.machine = message_board::MachineLabel::try_from("second import machine".to_owned())?;
        changed.declared_owner = message_board::HumanId::try_from("second-importer".to_owned())?;
        assert_eq!(
            serde_json::to_value(request(
                &reopened,
                &AdminProjectImportRequest::Begin { header: changed }
            )
            .await?)?,
            serde_json::json!({"kind":"refused","value":{"kind":"importAttemptInProgress","value":{
                "provenance":{"installation":"550e8400-e29b-41d4-a716-446655440000","machine":"original import machine"},
                "declaredOwner":"first-importer"
            }}})
        );
        assert_eq!(durable_rows(directory.path(), &copy, page_sql).await?, pages_before);
        assert_eq!(durable_rows(directory.path(), "fleet", workflow_sql).await?, workflow_before);
        assert_eq!(request(&reopened, &page).await?, staged);
        Ok(())
    }
    .await;
    let stopped = reopened.shutdown().await;
    let fixture_stopped = jwks.shutdown().await;
    stopped?;
    fixture_stopped?;
    observed
}
