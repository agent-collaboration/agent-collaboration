//! Template-derived bindings still pass through the real Worker's domain decoder.
use super::*;
pub(super) async fn run(settings: &WorkerdSettings) -> ConformanceResult<()> {
    for value in ["{}", "not-a-peer"] {
        let worker = WorkerdProcess::start_with_relay_value(settings, value).await?;
        let observed=async {
            let response=reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10)).build()?
                .post(format!("http://127.0.0.1:{}/admin/v1/machines/enroll",settings.port))
                .header("Cf-Access-Jwt-Assertion",collaboration_central_conformance::owner_assertion()?)
                .json(&serde_json::json!({"kind":"enrollMachine","value":{
                    "installation":"550e8400-e29b-41d4-a716-446655441040","peer":"019f0000-0000-7000-8000-000000001040","label":"invalid config enrollment",
                    "access_client_id":collaboration_central_conformance::access_client_id("invalid-config")?,
                    "credential_hash":"a".repeat(64),"expires_at":"2033-05-18T03:33:20Z"
                }})).send().await?;
            let status=response.status(); let body=response.json::<serde_json::Value>().await?;
            if status!=reqwest::StatusCode::SERVICE_UNAVAILABLE || body!=serde_json::json!("objectUnavailable") {
                return Err(format!("invalid template config initialized the Registry: {status}: {body}").into());
            }
            Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
        }.await;
        worker.stop().await?;
        observed?;
    }
    Ok(())
}
