//! Unknown publication outcomes expose bounded typed context, never credentials or payloads.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::native_fixture,
    offline_cli_fixture::{RunFixture, TestResult},
};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the compiled diagnostic without leaking response content."
)]
async fn compiled_publication_error_reports_substage_and_http_status_without_response_body()
-> TestResult {
    // Arrange an actual HTTP refusal at the first publication substage.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let target = format!("http://{}", listener.local_addr()?);
    let app = axum::Router::new().route(
        "/admin/v1/import/fleet",
        axum::routing::post(|| async {
            (
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "sensitive-response-marker",
            )
        }),
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _stopped = stopped.await;
            })
            .await
    });
    let result = async {
        let host = CutoverHostFixture::new(None)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let source = native_fixture(&host)
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let area = tempfile::tempdir()?;
        let run = RunFixture::new(&host, &source, &target, area.path())?;
        // Act through the signed compiled operator, keeping the actual reqwest boundary real.
        let output = run.execute().await?;
        assert!(!output.status.success());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        // Assert exact substage/cause while the source stays unarchived and no server body leaks.
        assert_eq!(report["kind"], "offlineIncomplete");
        assert_eq!(
            report["projects"][0]["diagnostic"]["stage"],
            serde_json::json!({"kind":"fleet"})
        );
        assert_eq!(
            report["projects"][0]["diagnostic"]["failure"],
            serde_json::json!({"kind":"httpStatus","status":429})
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("sensitive-response-marker"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("sensitive-response-marker"));
        assert!(source.path.exists());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let _stopped = stop.send(());
    server.await??;
    result
}
