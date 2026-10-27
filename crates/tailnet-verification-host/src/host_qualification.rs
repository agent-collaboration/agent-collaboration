use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::bounded_command::{command, output};
use crate::driver_error::{DriverError, DriverResult};
use crate::driver_records::{read_record, sha256};
use crate::host_plan::{HostPlan, HttpsEndpoint};
use crate::json_equality::same_json;
use crate::mcp_observation::LoopbackEndpoint;

pub fn require_hash(path: &Path, expected: &str) -> DriverResult<()> {
    if !path.is_absolute() || sha256(path)? != expected {
        return Err(DriverError::Invalid("artifact hash mismatch"));
    }
    Ok(())
}
#[derive(Deserialize)]
struct QualifiedArtifact {
    name: String,
    path: PathBuf,
    sha256: String,
    features: Vec<String>,
    version: String,
}
#[derive(Deserialize)]
struct Qualification {
    #[serde(rename = "sourceHead")]
    source_head: String,
    artifacts: Vec<QualifiedArtifact>,
}

pub fn qualify_artifacts(plan: &HostPlan) -> DriverResult<BTreeMap<String, PathBuf>> {
    require_hash(&plan.qualification, &plan.qualification_sha256)?;
    let qualified: Qualification = read_record(&plan.qualification)?;
    if qualified.source_head != plan.source_head {
        return Err(DriverError::Invalid("qualification source mismatch"));
    }
    let head = command(&[
        "git".to_owned(),
        "-C".to_owned(),
        plan.repository.to_string_lossy().into_owned(),
        "rev-parse".to_owned(),
        "HEAD".to_owned(),
    ])?;
    if std::str::from_utf8(&head)
        .map_err(|_| DriverError::Invalid("source HEAD UTF-8"))?
        .trim()
        != plan.source_head
    {
        return Err(DriverError::Invalid("source HEAD mismatch"));
    }
    let count = qualified.artifacts.len();
    let mut binaries = BTreeMap::new();
    for artifact in qualified.artifacts {
        if !["codex-router", "agent-collaboration", "agent-sessions"]
            .contains(&artifact.name.as_str())
        {
            return Err(DriverError::Invalid("unexpected qualification artifact"));
        }
        require_hash(&artifact.path, &artifact.sha256)?;
        if artifact.features.iter().any(|feature| feature != "default")
            || artifact.version != format!("{} {}", artifact.name, plan.expected_version)
        {
            return Err(DriverError::Invalid("not the declared normal artifact"));
        }
        command(&[
            "/usr/bin/codesign".to_owned(),
            "--verify".to_owned(),
            "--strict".to_owned(),
            artifact.path.to_string_lossy().into_owned(),
        ])?;
        let signing = output(
            &[
                "/usr/bin/codesign".to_owned(),
                "-dv".to_owned(),
                "--verbose=4".to_owned(),
                artifact.path.to_string_lossy().into_owned(),
            ],
            Duration::from_secs(10),
            true,
        )?;
        let signing = std::str::from_utf8(&signing.stderr)
            .map_err(|_| DriverError::Invalid("signing UTF-8"))?;
        if !signing.contains(&format!(
            "Identifier=dev.shravansunder.{}.debug",
            artifact.name
        )) || !signing.contains("TeamIdentifier=974QD84WVC")
        {
            return Err(DriverError::Invalid("debug signing identity mismatch"));
        }
        let version = command(&[
            artifact.path.to_string_lossy().into_owned(),
            "--version".to_owned(),
        ])?;
        if std::str::from_utf8(&version)
            .map_err(|_| DriverError::Invalid("version UTF-8"))?
            .trim()
            != artifact.version
        {
            return Err(DriverError::Invalid("actual artifact version mismatch"));
        }
        binaries.insert(artifact.name, artifact.path);
    }
    if count != 3 || binaries.len() != 3 {
        return Err(DriverError::Invalid(
            "three distinct normal artifacts are required",
        ));
    }
    Ok(binaries)
}

pub fn serve_snapshot(tailscale: &Path) -> DriverResult<Value> {
    let value: Value = serde_json::from_slice(&command(&[
        tailscale.to_string_lossy().into_owned(),
        "serve".to_owned(),
        "status".to_owned(),
        "--json".to_owned(),
    ])?)?;
    if !value.is_object() {
        return Err(DriverError::Invalid("Serve status must be an object"));
    }
    Ok(value)
}
#[must_use]
pub fn contains_port(value: &Value, port: u16) -> bool {
    value.as_object().is_some_and(|fields| {
        fields.iter().any(|(key, child)| {
            key == &port.to_string()
                || key.ends_with(&format!(":{port}"))
                || contains_port(child, port)
        })
    })
}
fn unchanged_subset(previous: &Value, current: &Value) -> bool {
    match (previous.as_object(), current.as_object()) {
        (Some(previous), Some(current)) => previous.iter().all(|(key, value)| {
            current
                .get(key)
                .is_some_and(|next| unchanged_subset(value, next))
        }),
        _ => same_json(previous, current),
    }
}
fn target_funnel(value: &Value, port: u16) -> bool {
    value.as_object().is_some_and(|fields| {
        fields
            .get("AllowFunnel")
            .and_then(Value::as_object)
            .is_some_and(|funnel| {
                funnel.iter().any(|(key, enabled)| {
                    enabled == &Value::Bool(true)
                        && (key == &port.to_string() || key.ends_with(&format!(":{port}")))
                })
            })
            || fields.values().any(|child| target_funnel(child, port))
    })
}
pub fn require_serve_during(before: &Value, during: &Value, port: u16) -> DriverResult<()> {
    if contains_port(before, port)
        || !contains_port(during, port)
        || !unchanged_subset(before, during)
        || target_funnel(during, port)
    {
        return Err(DriverError::Invalid(
            "Serve collision, unrelated overwrite, missing exposure or public Funnel",
        ));
    }
    Ok(())
}
pub fn require_serve_restored(before: &Value, after: &Value) -> DriverResult<()> {
    if !same_json(before, after) {
        return Err(DriverError::Invalid(
            "Serve state was not restored exactly; no reset or blind replay permitted",
        ));
    }
    Ok(())
}

pub fn require_http_serve_backend(
    during: &Value,
    local: &LoopbackEndpoint,
    remote: &HttpsEndpoint,
) -> DriverResult<String> {
    let foreground =
        during
            .get("Foreground")
            .and_then(Value::as_object)
            .ok_or(DriverError::Invalid(
                "owned HTTP foreground Serve session absent",
            ))?;
    let authority = remote.authority()?;
    let parsed = remote.parsed()?;
    let port = parsed
        .port_or_known_default()
        .ok_or(DriverError::Invalid("HTTPS port absent"))?
        .to_string();
    let origin = local.origin()?;
    let mut matches = Vec::new();
    for (session, config) in foreground {
        let (Some(tcp), Some(web)) = (
            config.get("TCP").and_then(Value::as_object),
            config.get("Web").and_then(Value::as_object),
        ) else {
            continue;
        };
        let funnel = config.get("AllowFunnel");
        if funnel.is_some_and(|value| !value.is_object()) {
            continue;
        }
        let https = tcp.get(&port).and_then(|listener| listener.get("HTTPS"));
        let proxy = web
            .get(&authority)
            .and_then(|endpoint| endpoint.get("Handlers"))
            .and_then(|handlers| handlers.get("/"))
            .and_then(|handler| handler.get("Proxy"))
            .and_then(Value::as_str);
        let private = funnel
            .and_then(|value| value.get(&authority))
            .is_none_or(|value| value == &Value::Bool(false));
        if https == Some(&Value::Bool(true))
            && proxy.is_some_and(|value| value.trim_end_matches('/') == origin)
            && private
            && !session.is_empty()
        {
            matches.push(session.clone());
        }
    }
    let [session] = matches.as_slice() else {
        return Err(DriverError::Invalid(
            "expected exactly one private foreground HTTP backend matching the actual MCP listener",
        ));
    };
    Ok(session.clone())
}
