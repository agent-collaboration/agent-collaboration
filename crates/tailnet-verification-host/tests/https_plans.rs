use codex_router_tailnet_verification_host::host_plan::HttpsEndpoint;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct UrlCase {
    url: String,
    expected: Value,
}
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert the contract; propagate fixture errors."
)]
fn https_urls_match_the_original_literal_path_and_authority_contract()
-> Result<(), Box<dyn std::error::Error>> {
    let cases: Vec<UrlCase> = serde_json::from_str(include_str!(
        "../../../verification/observations/tests/baselines/tailnet-urls.json"
    ))?;
    for case in cases {
        let endpoint = HttpsEndpoint { url: case.url };
        if case.expected.get("refused") == Some(&Value::Bool(true)) {
            assert!(
                endpoint.parsed().is_err(),
                "{} must be refused",
                endpoint.url
            );
        } else {
            assert_eq!(
                json!({"authority":endpoint.authority()?,"origin":endpoint.origin()?}),
                case.expected
            );
        }
    }
    Ok(())
}
