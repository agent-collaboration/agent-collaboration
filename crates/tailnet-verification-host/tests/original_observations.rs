#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions fail these tests while fixture errors propagate through Result."
)]

use std::path::Path;

use codex_router_tailnet_verification_host::driver_error::{DriverError, DriverResult};
use codex_router_tailnet_verification_host::host_lifecycle::host_readiness_until;
use codex_router_tailnet_verification_host::host_plan::{HttpsEndpoint, validate_ports};
use codex_router_tailnet_verification_host::host_qualification::{
    require_http_serve_backend, require_serve_during, require_serve_restored,
};
use codex_router_tailnet_verification_host::mcp_observation::{
    HttpReply, LoopbackEndpoint, NativeReadiness, observe_native_readiness, parse_manifest_mcp,
    require_service, rpc_result,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
struct OracleCase {
    function: String,
    args: Vec<Value>,
    expected: Value,
}
fn argument(case: &OracleCase, index: usize) -> DriverResult<&Value> {
    case.args
        .get(index)
        .ok_or(DriverError::Invalid("oracle argument missing"))
}
fn text(value: &Value) -> DriverResult<&str> {
    value
        .as_str()
        .ok_or(DriverError::Invalid("oracle text missing"))
}
fn port(value: &Value) -> DriverResult<u16> {
    value
        .as_u64()
        .and_then(|value| u16::try_from(value).ok())
        .ok_or(DriverError::Invalid("port out of range"))
}
fn invoke(case: &OracleCase) -> DriverResult<Value> {
    let first = argument(case, 0)?;
    match case.function.as_str() {
        "parse_manifest_mcp" => {
            Ok(json!({"url":parse_manifest_mcp(first)?.url,"transport":"streamableHttp"}))
        }
        "validate_ports" => {
            validate_ports(port(first)?, port(argument(case, 1)?)?)?;
            Ok(Value::Null)
        }
        "observe_native_readiness" => Ok(json!(match observe_native_readiness(
            first,
            text(argument(case, 1)?)?,
            text(argument(case, 2)?)?
        )? {
            NativeReadiness::Starting => "starting",
            NativeReadiness::Ready => "ready",
        })),
        "require_service" => {
            require_service(
                first,
                text(argument(case, 1)?)?,
                case.args.get(2).map(text).transpose()?,
            )?;
            Ok(Value::Null)
        }
        "require_serve_during" => {
            require_serve_during(first, argument(case, 1)?, port(argument(case, 2)?)?)?;
            Ok(Value::Null)
        }
        "require_serve_restored" => {
            require_serve_restored(first, argument(case, 1)?)?;
            Ok(Value::Null)
        }
        "require_http_serve_backend" => {
            let local = LoopbackEndpoint::parse(
                text(
                    argument(case, 1)?
                        .get("url")
                        .ok_or(DriverError::Invalid("local URL absent"))?,
                )?
                .to_owned(),
            )?;
            let remote: HttpsEndpoint = serde_json::from_value(argument(case, 2)?.clone())?;
            Ok(json!(require_http_serve_backend(first, &local, &remote)?))
        }
        "rpc_result" => rpc_result(HttpReply {
            status: port(
                first
                    .get("status")
                    .ok_or(DriverError::Invalid("HTTP status absent"))?,
            )?,
            body: text(
                first
                    .get("body")
                    .ok_or(DriverError::Invalid("HTTP body absent"))?,
            )?
            .as_bytes()
            .to_vec(),
        }),
        _ => Err(DriverError::Invalid("unhandled oracle function")),
    }
}

#[test]
fn matches_forty_two_original_observation_executions() -> Result<(), Box<dyn std::error::Error>> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/original-observations");
    let mut files = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    files.sort_by_key(std::fs::DirEntry::file_name);
    assert_eq!(files.len(), 42);
    for file in files {
        let case: OracleCase = serde_json::from_slice(&std::fs::read(file.path())?)?;
        let result = invoke(&case);
        if case.expected.get("refused") == Some(&Value::Bool(true)) {
            assert!(
                result.is_err(),
                "{} accepted rejected original input: {}",
                case.function,
                file.path().display()
            );
        } else {
            assert_eq!(
                result?,
                case.expected
                    .get("result")
                    .cloned()
                    .ok_or("expected result absent")?,
                "{} differs: {}",
                case.function,
                file.path().display()
            );
        }
    }
    Ok(())
}

#[test]
fn original_readiness_bound_includes_cold_start_and_never_extends_the_allocation() {
    for (remaining, expected) in [
        (1000, 1150.0),
        (150, 1150.0),
        (45, 1045.0),
        (0, 1000.0),
        (-1, 999.0),
    ] {
        assert_eq!(
            host_readiness_until(chrono::TimeDelta::seconds(remaining), 1000.0),
            expected
        );
    }
}
