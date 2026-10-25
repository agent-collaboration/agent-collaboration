//! The standard listener retains only the core's sanitized Access classifications.
#![cfg(feature = "conformance")]
#![allow(
    clippy::panic_in_result_fn,
    reason = "Literal assertions qualify permanent conformance cases."
)]
#[path = "native_http/support.rs"]
mod configuration;

use collaboration_central_conformance::{JwksFixtureServer, fixture_policy, owner_assertion};
use collaboration_central_tokio::{DeliveryConfig, TokioServer, TokioSettings};
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);
impl Write for CapturedLogs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| std::io::Error::other("log capture poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn standard_server_logs_accepted_and_refused_access_without_credentials()
-> collaboration_central_conformance::ConformanceResult<()> {
    let logs = CapturedLogs::default();
    let output = logs.clone();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .without_time()
            .with_writer(move || output.clone())
            .finish(),
    )?;
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let server = TokioServer::start(TokioSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
        registry: configuration::registry_config()?,
        delivery: DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
    })
    .await?;
    let assertion = owner_assertion()?;
    let proof = async {
        let client = reqwest::Client::builder().no_proxy().build()?;
        client
            .get(format!("{}/admin/v1/copies", server.base_url()))
            .header("Cf-Access-Jwt-Assertion", &assertion)
            .send()
            .await?
            .error_for_status()?;
        let refused = client
            .get(format!("{}/v1/session", server.base_url()))
            .send()
            .await?;
        assert_eq!(refused.status(), reqwest::StatusCode::UNAUTHORIZED);
        let captured =
            String::from_utf8(logs.0.lock().map_err(|_| "log capture poisoned")?.clone())?;
        let events = captured
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()?;
        assert!(
            events
                .iter()
                .any(|event| event.pointer("/fields/route_class")
                    == Some(&serde_json::json!("Admin"))
                    && event.pointer("/fields/outcome")
                        == Some(&serde_json::json!("AcceptedOwner"))
                    && event.pointer("/fields/key_source")
                        == Some(&serde_json::json!("Some(Fetched)"))),
            "accepted classifications absent: {captured}"
        );
        assert!(
            events
                .iter()
                .any(|event| event.pointer("/fields/route_class")
                    == Some(&serde_json::json!("Session"))
                    && event
                        .pointer("/fields/outcome")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|value| value.starts_with("Refused("))
                    && event.pointer("/fields/key_source") == Some(&serde_json::json!("None"))),
            "refused classifications absent: {captured}"
        );
        assert!(!captured.contains(&assertion));
        assert!(!captured.contains(root.path().to_string_lossy().as_ref()));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    server.shutdown().await?;
    jwks.shutdown().await?;
    proof
}
