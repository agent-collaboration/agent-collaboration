use super::{PublicationError, PublicationStage};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A held real HTTP response must report a request timeout without its URL."
)]
async fn request_timeout_diagnostic_keeps_stage_and_omits_the_request_url() -> TestResult {
    // Arrange a real accepted connection that never receives response headers before the deadline.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/credential-marker", listener.local_addr()?);
    let (release, held) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (connection, _) = listener.accept().await?;
        let _held = held.await;
        drop(connection);
        Ok::<_, std::io::Error>(())
    });
    // Act through the same reqwest timeout boundary, with a short test-owned deadline.
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_millis(100))
        .build()?;
    let request = client.get(url).send().await;
    let _released = release.send(());
    server.await??;
    let Err(source) = request else {
        return Err("held request did not time out".into());
    };
    let error = PublicationError::OutcomeUnknown {
        stage: PublicationStage::Fleet,
        source,
    };
    let diagnostic = serde_json::to_value(error.diagnostic())?;
    // Assert no raw URL or error/source text is serialized.
    assert_eq!(diagnostic["stage"], serde_json::json!({"kind":"fleet"}));
    assert_eq!(
        diagnostic["failure"],
        serde_json::json!({"kind":"timeout","during":"request"})
    );
    assert!(!serde_json::to_string(&diagnostic)?.contains("credential-marker"));
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Malformed HTTP bodies must be classified without copying their content."
)]
async fn response_decode_diagnostic_keeps_the_page_index_and_omits_body_text() -> TestResult {
    // Arrange a complete HTTP response whose body cannot be a Central import answer.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/", listener.local_addr()?);
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let app = axum::Router::new().route(
        "/",
        axum::routing::get(|| async { "sensitive-response-marker" }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _stopped = stopped.await;
            })
            .await
    });
    // Act through actual response JSON decoding.
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(url)
        .send()
        .await?;
    let decoded = response
        .json::<collaboration_central_core::AdminProjectImportAnswer>()
        .await;
    let _stopped = stop.send(());
    server.await??;
    let Err(source) = decoded else {
        return Err("malformed response unexpectedly decoded".into());
    };
    let error = PublicationError::OutcomeUnknown {
        stage: PublicationStage::Page { index: 7 },
        source,
    };
    let diagnostic = serde_json::to_value(error.diagnostic())?;
    // Assert exact stage correlation and no body leakage.
    assert_eq!(
        diagnostic["stage"],
        serde_json::json!({"kind":"page","index":7})
    );
    assert_eq!(diagnostic["failure"], serde_json::json!({"kind":"decode"}));
    assert!(!serde_json::to_string(&diagnostic)?.contains("sensitive-response-marker"));
    Ok(())
}
