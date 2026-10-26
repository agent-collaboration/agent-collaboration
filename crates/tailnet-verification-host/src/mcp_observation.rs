use std::io::Read;
use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::bounded_command::require_continuing;
use crate::driver_error::{DriverError, DriverResult};
use crate::host_plan::{HttpsEndpoint, literal_mcp_path};
use crate::json_equality::same_json;

#[derive(Clone, Debug)]
pub struct LoopbackEndpoint {
    pub url: String,
}

impl LoopbackEndpoint {
    pub fn parse(url: String) -> DriverResult<Self> {
        let parsed = reqwest::Url::parse(&url)
            .map_err(|_| DriverError::Invalid("expected explicit loopback HTTP MCP endpoint"))?;
        let authority = url
            .split_once("://")
            .and_then(|(_, tail)| tail.split('/').next())
            .unwrap_or_default();
        let port = authority
            .rsplit_once(':')
            .and_then(|(_, value)| value.parse::<u16>().ok());
        let loopback = parsed
            .host_str()
            .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            .is_some_and(|address| address.is_loopback());
        if parsed.scheme() != "http"
            || !loopback
            || port.is_none_or(|value| value == 0)
            || parsed.path() != "/mcp" || !literal_mcp_path(&url)
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(DriverError::Invalid(
                "expected explicit loopback HTTP MCP endpoint",
            ));
        }
        Ok(Self { url })
    }
    pub fn origin(&self) -> DriverResult<String> {
        let authority = self
            .url
            .split_once("://")
            .and_then(|(_, tail)| tail.split('/').next())
            .ok_or(DriverError::Invalid("MCP authority absent"))?;
        Ok(format!("http://{authority}"))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpSelector {
    transport: String,
    url: String,
}
pub fn parse_manifest_mcp(manifest: &Value) -> DriverResult<LoopbackEndpoint> {
    let selector: McpSelector =
        serde_json::from_value(manifest.get("mcp").cloned().unwrap_or(Value::Null))?;
    if selector.transport != "streamableHttp" {
        return Err(DriverError::Invalid("unexpected MCP transport"));
    }
    LoopbackEndpoint::parse(selector.url)
}

pub struct HttpReply {
    pub status: u16,
    pub body: Vec<u8>,
}
#[derive(Default)]
pub struct ProbeHeaders {
    pub origin: Option<String>,
    pub host_authority: Option<String>,
}

pub enum LocalTarget<'target> {
    Loopback(&'target LoopbackEndpoint),
    Unix(&'target Path),
}
pub fn http_post(
    target: LocalTarget<'_>,
    payload: &Value,
    headers: &ProbeHeaders,
) -> DriverResult<HttpReply> {
    require_continuing()?;
    let builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .http1_only()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .pool_max_idle_per_host(0);
    let (builder, url) = match target {
        LocalTarget::Loopback(endpoint) => (builder, endpoint.url.as_str()),
        LocalTarget::Unix(path) => (builder.unix_socket(path), "http://localhost/mcp"),
    };
    let client = builder.build()?;
    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-11-25")
        .body(serde_json::to_vec(payload)?);
    if let Some(origin) = &headers.origin {
        request = request.header("Origin", origin);
    }
    if let Some(host) = &headers.host_authority {
        request = request.header("Host", host);
    }
    let response = request.send()?;
    let status = response.status().as_u16();
    let mut body = Vec::new();
    response.take(4 * 1024 * 1024 + 1).read_to_end(&mut body)?;
    if body.len() > 4 * 1024 * 1024 {
        return Err(DriverError::Invalid(
            "HTTP response exceeded evidence bound",
        ));
    }
    require_continuing()?;
    Ok(HttpReply { status, body })
}

pub fn rpc_result(reply: HttpReply) -> DriverResult<Value> {
    let envelope: Value = serde_json::from_slice(&reply.body)?;
    if reply.status != 200
        || envelope.get("jsonrpc") != Some(&json!("2.0"))
        || !envelope
            .get("id")
            .is_some_and(|id| same_json(id, &json!(1)))
        || envelope.get("error").is_some()
    {
        return Err(DriverError::Invalid("MCP HTTP/error/correlation failure"));
    }
    let result = envelope
        .get("result")
        .filter(|value| value.is_object())
        .ok_or(DriverError::Invalid("MCP result must be an object"))?;
    if result.get("isError") == Some(&Value::Bool(true)) {
        return Err(DriverError::Invalid(
            "MCP tool returned an error; no mutation replay",
        ));
    }
    Ok(result.clone())
}
pub fn tool_payload(name: &str, arguments: &Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}})
}
pub fn tool_result(reply: HttpReply) -> DriverResult<Value> {
    rpc_result(reply)?
        .get("structuredContent")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or(DriverError::Invalid(
            "MCP structured content must be an object",
        ))
}
pub fn tool(socket: &Path, name: &str, arguments: &Value) -> DriverResult<Value> {
    tool_result(http_post(
        LocalTarget::Unix(socket),
        &tool_payload(name, arguments),
        &ProbeHeaders::default(),
    )?)
}

pub fn require_service(
    inventory: &Value,
    service_id: &str,
    epoch: Option<&str>,
) -> DriverResult<()> {
    if epoch.is_some_and(|epoch| inventory.get("serviceEpoch") != Some(&json!(epoch))) {
        return Err(DriverError::Invalid("destination service epoch changed"));
    }
    let entries = inventory
        .get("endpoints")
        .and_then(Value::as_array)
        .filter(|entries| !entries.is_empty())
        .ok_or(DriverError::Invalid("endpoint inventory absent"))?;
    for item in entries {
        if item.pointer("/endpoint/serviceId") != Some(&json!(service_id)) {
            return Err(DriverError::Invalid("wrong destination service"));
        }
    }
    let native: Vec<_> = entries
        .iter()
        .filter(|item| {
            item.get("endpoint")
                == Some(&json!({"serviceId":service_id,"endpointId":"codex-local"}))
        })
        .collect();
    let [native] = native.as_slice() else {
        return Err(DriverError::Invalid("native endpoint is not available"));
    };
    if native.pointer("/availability/state") != Some(&json!("available")) {
        return Err(DriverError::Invalid("native endpoint is not available"));
    }
    let channels = native
        .get("channels")
        .and_then(Value::as_array)
        .ok_or(DriverError::Invalid("native generation missing"))?;
    if !channels.iter().any(|item| {
        item.get("kind") == Some(&json!("nativeCodex"))
            && item.get("generation").is_some_and(Value::is_object)
    }) {
        return Err(DriverError::Invalid("native generation missing"));
    }
    for channel in channels
        .iter()
        .filter(|item| item.get("kind") == Some(&json!("nativeCodex")))
    {
        let observed_epoch = channel
            .pointer("/generation/serviceEpoch")
            .and_then(Value::as_str);
        if observed_epoch.is_none()
            || epoch.is_some_and(|epoch| observed_epoch != Some(epoch))
            || !channel
                .pointer("/generation/generation")
                .is_some_and(|value| value.is_i64() || value.is_u64())
        {
            return Err(DriverError::Invalid(
                "native endpoint epoch differs from the declared Host",
            ));
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum NativeReadiness {
    Starting,
    Ready,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartingIdentity {
    #[serde(rename = "serviceId")]
    service_id: String,
    #[serde(rename = "endpointId")]
    endpoint_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisabledState {
    state: String,
    #[serde(rename = "observedAt")]
    observed_at: String,
    reason: String,
    fix: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartingEndpoint {
    endpoint: StartingIdentity,
    label: String,
    availability: DisabledState,
    channels: Vec<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartingInventory {
    #[serde(rename = "serviceEpoch")]
    service_epoch: String,
    sequence: u64,
    endpoints: Vec<StartingEndpoint>,
}

pub fn observe_native_readiness(
    inventory: &Value,
    service_id: &str,
    epoch: &str,
) -> DriverResult<NativeReadiness> {
    let native_present = inventory
        .get("endpoints")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.pointer("/endpoint/endpointId") == Some(&json!("codex-local")))
        });
    if native_present {
        require_service(inventory, service_id, Some(epoch))?;
        return Ok(NativeReadiness::Ready);
    }
    let starting: StartingInventory = serde_json::from_value(inventory.clone())?;
    if starting.service_epoch != epoch {
        return Err(DriverError::Invalid("destination service epoch changed"));
    }
    let _sequence = starting.sequence;
    let mut providers = std::collections::BTreeSet::new();
    for entry in &starting.endpoints {
        if entry.endpoint.service_id != service_id {
            return Err(DriverError::Invalid("wrong destination service"));
        }
        let state = &entry.availability;
        if entry.endpoint.service_id.is_empty()
            || entry.label.is_empty()
            || !entry.channels.is_empty()
            || state.state != "unavailable"
            || state.reason != "disabled in providers.json"
            || state.fix.is_empty()
            || !state.observed_at.ends_with('Z')
            || chrono::DateTime::parse_from_rfc3339(&state.observed_at).is_err()
        {
            return Err(DriverError::Invalid("invalid disabled provider startup"));
        }
        providers.insert(entry.endpoint.endpoint_id.as_str());
    }
    if starting.endpoints.len() != 2
        || providers != std::collections::BTreeSet::from(["claude-local", "cursor-local"])
    {
        return Err(DriverError::Invalid(
            "starting inventory must name both disabled providers exactly once",
        ));
    }
    Ok(NativeReadiness::Starting)
}

pub fn observe_local_inventory(
    local: &LoopbackEndpoint,
    remote: &HttpsEndpoint,
    invocation: (&str, &str),
) -> DriverResult<Value> {
    let inventory = tool_result(http_post(
        LocalTarget::Loopback(local),
        &tool_payload("endpoints_list", &json!({})),
        &ProbeHeaders {
            host_authority: Some(remote.authority()?),
            origin: None,
        },
    )?)?;
    require_service(&inventory, invocation.0, Some(invocation.1))?;
    Ok(inventory)
}

pub fn observe_http_boundary(
    local: &LoopbackEndpoint,
    remote: &HttpsEndpoint,
) -> DriverResult<Value> {
    let parsed = remote.parsed()?;
    let port = parsed
        .port_or_known_default()
        .ok_or(DriverError::Invalid("remote port missing"))?;
    let wrong_port = if port == 65535 { 1 } else { port + 1 };
    let authority = remote.authority()?;
    let host = parsed
        .host_str()
        .ok_or(DriverError::Invalid("remote host missing"))?;
    let probes = [
        ("loopbackControl", None, None, 200),
        ("existingLoopbackOrigin", Some(local.origin()?), None, 200),
        ("proxyAuthorityControl", None, Some(authority.clone()), 200),
        (
            "loopbackOriginThroughProxyAuthority",
            Some(local.origin()?),
            Some(authority.clone()),
            200,
        ),
        (
            "proxyHttpOrigin",
            Some(format!("http://{authority}")),
            Some(authority.clone()),
            403,
        ),
        (
            "wrongHost",
            None,
            Some(format!("unexpected.invalid:{port}")),
            403,
        ),
        ("wrongPort", None, Some(format!("{host}:{wrong_port}")), 403),
        (
            "missingPort",
            None,
            Some(host.to_owned()),
            if port == 443 { 200 } else { 403 },
        ),
        (
            "tailnetOrigin",
            Some(remote.origin()?),
            Some(authority),
            403,
        ),
    ];
    let mut statuses = serde_json::Map::new();
    for (name, origin, host_authority, expected) in probes {
        let response = http_post(
            LocalTarget::Loopback(local),
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
            &ProbeHeaders {
                origin,
                host_authority,
            },
        )?;
        if response.status != expected {
            return Err(DriverError::Invalid("HTTP Host/Origin boundary failed"));
        }
        if expected == 200 {
            rpc_result(response)?;
        }
        statuses.insert(name.to_owned(), json!(expected));
    }
    Ok(Value::Object(statuses))
}
