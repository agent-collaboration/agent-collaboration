//! A sealed artifact cannot publish old content after the offline source changes.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::{NativeFixture, fixture_database, native_fixture},
    offline_cli_fixture::{RunFixture, TestResult},
    recovery_rows::require,
};
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::post,
};
use board_central_test_support::TestCentral;
use board_staging_cutover::PreparedProjectBundle;
use message_board::{ActorOrigin, Record, RequestId, TrailSubject, WorkTrailRow, WorkTrailRowId};
use message_board_storage::ProjectStore;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{sync::oneshot, task::JoinHandle};

#[tokio::test]
async fn saved_bundle_refuses_an_advanced_source_before_retrying_any_import() -> TestResult {
    // Arrange a real native source and real target, with one failed publication response.
    let host = CutoverHostFixture::new(None).map_err(boxed)?;
    let source = native_fixture(&host).await.map_err(boxed)?;
    let origin = ActorOrigin {
        installation: String::from(host.service.clone()).try_into()?,
        machine: "saved bundle resume regression".to_owned().try_into()?,
    };
    let target_area = tempfile::tempdir()?;
    let central = TestCentral::start_with_peer(target_area.path(), &origin, host.peer).await?;
    let proxy = FailFirstPublication::start(central.base_url()).await?;
    let result = async {
        let artifacts = tempfile::tempdir()?;
        let run = RunFixture::new(&host, &source, &proxy.endpoint, artifacts.path())?;
        let failed = run.execute().await?;
        require(
            !failed.status.success(),
            "the initial publication must fail",
        )?;
        let bundle = PreparedProjectBundle::open(&run.bundle)?;
        let sealed = std::fs::read(run.bundle.join("manifest.json"))?;
        let before = proxy.requests.load(Ordering::SeqCst);
        require(
            before == 1,
            "the initial failed publication must not reach the target",
        )?;
        require(
            central.copy_status(&source.selected.project).await.is_err(),
            "initial target was already imported",
        )?;
        advance(&host, &source).await?;
        require(
            !bundle
                .manifest()
                .source_fingerprint
                .matches(&bundle.manifest().source_directory)?,
            "real source advance did not change its sealed fingerprint",
        )?;
        run.renew_owner(&proxy.endpoint)?;
        // Act: the same immutable plan is retried after real local project admission.
        let retried = run.execute().await?;
        // Assert remote custody is untouched, not merely that archive eventually refuses.
        require(
            proxy.requests.load(Ordering::SeqCst) == before,
            "stale saved bundle reached remote import before the source-change refusal",
        )?;
        require(!retried.status.success(), "changed source retry succeeded")?;
        let report: serde_json::Value = serde_json::from_slice(&retried.stdout)?;
        let projects = report
            .get("projects")
            .and_then(serde_json::Value::as_array)
            .ok_or("project outcome missing")?;
        let [project] = projects.as_slice() else {
            return Err("one project outcome required".into());
        };
        require(
            project.get("reason").and_then(serde_json::Value::as_str)
                == Some("the source directory changed after immutable preparation"),
            "retry did not report SourceChanged",
        )?;
        require(
            central.copy_status(&source.selected.project).await.is_err(),
            "stale project was imported",
        )?;
        require(
            bundle.manifest().source_directory.is_dir()
                && !bundle.manifest().backup_directory.try_exists()?,
            "changed source was moved or archived",
        )?;
        require(
            std::fs::read(run.bundle.join("manifest.json"))? == sealed,
            "retry rewrote the immutable bundle",
        )?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = proxy.shutdown().await;
    let central_stopped = central.shutdown().await;
    result?;
    stopped?;
    central_stopped?;
    Ok(())
}

async fn advance(host: &CutoverHostFixture, source: &NativeFixture) -> TestResult {
    let database = Arc::new(
        fixture_database(&source.path, source.manifest.database, host.peer)
            .await
            .map_err(boxed)?,
    );
    let store =
        ProjectStore::from_database(source.selected.project.clone(), database.clone()).await?;
    let mut write = store
        .begin_admission(ActorOrigin {
            installation: String::from(host.service.clone()).try_into()?,
            machine: "source advanced after failed publication"
                .to_owned()
                .try_into()?,
        })
        .await?;
    let Some(Record::Project(project)) = write
        .record(&source.selected.project.clone().into())
        .await?
    else {
        return Err("source project absent".into());
    };
    let row_id = WorkTrailRowId::generate();
    let row = WorkTrailRow {
        envelope: message_board::RecordEnvelope {
            id: row_id.clone().into(),
            request: Some(RequestId::generate()),
            ..project.envelope
        },
        row_id,
        project_id: source.selected.project.clone(),
        subject: TrailSubject::Project,
        text: "new content after the frozen bundle's publication failed"
            .to_owned()
            .try_into()?,
        tags: vec![],
        links: vec![],
        redacted: false,
    };
    write.append_record(Record::WorkTrailRow(row)).await?;
    write.commit().await?;
    drop(store);
    database.close().await;
    drop(database);
    Ok(())
}
fn boxed(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}

#[derive(Clone)]
struct Forwarding {
    upstream: String,
    client: reqwest::Client,
    requests: Arc<AtomicUsize>,
}
pub(super) struct FailFirstPublication {
    pub endpoint: String,
    pub requests: Arc<AtomicUsize>,
    stop: oneshot::Sender<()>,
    running: JoinHandle<std::io::Result<()>>,
}
impl FailFirstPublication {
    pub async fn start(upstream: &str) -> TestResult<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(AtomicUsize::new(0));
        let state = Forwarding {
            upstream: upstream.to_owned(),
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(20))
                .build()?,
            requests: requests.clone(),
        };
        let app = Router::new()
            .route("/admin/v1/import/fleet", post(forward))
            .route("/admin/v1/import/projects/{project}", post(forward))
            .with_state(state);
        let (stop, stopped) = oneshot::channel();
        let running = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        Ok(Self {
            endpoint,
            requests,
            stop,
            running,
        })
    }
    pub async fn shutdown(self) -> TestResult {
        let _ = self.stop.send(());
        self.running.await??;
        Ok(())
    }
}
async fn forward(
    State(state): State<Forwarding>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if state.requests.fetch_add(1, Ordering::SeqCst) == 0 {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let result = async {
        let mut request = state
            .client
            .post(format!("{}{}", state.upstream, uri.path()))
            .header("Content-Type", "application/json")
            .body(body);
        for name in ["Cf-Access-Jwt-Assertion", "cf-access-token"] {
            if let Some(value) = headers.get(name) {
                request = request.header(name, value.clone());
            }
        }
        let response = request.send().await?;
        let status = response.status();
        let body = response.bytes().await?;
        Ok::<_, reqwest::Error>((status, body).into_response())
    }
    .await;
    result.unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}
