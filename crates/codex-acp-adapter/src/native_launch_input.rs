//! Project only declared launch input; scratch/profile values and starter authority are separate.
use crate::{AcpSchemaCatalog, McpConfiguration};
use collaboration_contracts::{
    HarnessDeclaredLaunchInput, HarnessDeclaredSettings, HarnessForkInput,
};
use collaboration_protocol::{EndpointRef, SessionRef};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("native declared launch input is invalid")]
pub struct InvalidNativeLaunchInput;

fn required<TValue: DeserializeOwned>(
    fields: &Map<String, Value>,
    name: &str,
) -> Result<TValue, InvalidNativeLaunchInput> {
    serde_json::from_value(fields.get(name).cloned().ok_or(InvalidNativeLaunchInput)?)
        .map_err(|_| InvalidNativeLaunchInput)
}
fn optional<TValue: DeserializeOwned>(
    fields: &Map<String, Value>,
    name: &str,
) -> Result<Option<TValue>, InvalidNativeLaunchInput> {
    fields
        .get(name)
        .map(|value| serde_json::from_value(value.clone()).map_err(|_| InvalidNativeLaunchInput))
        .transpose()
        .map(Option::flatten)
}

pub fn native_declared_launch_input(
    catalog: &mut AcpSchemaCatalog,
    params: &Value,
    endpoint: &EndpointRef,
) -> Result<HarnessDeclaredLaunchInput, InvalidNativeLaunchInput> {
    if !catalog
        .validate("NewSessionRequest", params)
        .map_err(|_| InvalidNativeLaunchInput)?
    {
        return Err(InvalidNativeLaunchInput);
    }
    let fields = params.as_object().ok_or(InvalidNativeLaunchInput)?;
    let router = params
        .pointer("/_meta/codexRouter")
        .and_then(Value::as_object)
        .ok_or(InvalidNativeLaunchInput)?;
    let servers = params
        .get("mcpServers")
        .and_then(Value::as_array)
        .ok_or(InvalidNativeLaunchInput)?;
    let configuration =
        McpConfiguration::parse(catalog, servers).map_err(|_| InvalidNativeLaunchInput)?;
    let created_by: SessionRef = required(router, "createdBy")?;
    let approver: SessionRef = required(router, "approver")?;
    let fork = optional(router, "forkThreadId")?.map(|session_id| HarnessForkInput {
        source: SessionRef {
            endpoint: endpoint.clone(),
            session_id,
        },
        through_turn_id: None,
    });
    Ok(HarnessDeclaredLaunchInput {
        project_id: optional(router, "projectId")?,
        endpoint: endpoint.clone(),
        working_directory: required(fields, "cwd")?,
        settings: HarnessDeclaredSettings::Native {
            model: optional(router, "model")?,
            effort: optional(router, "effort")?,
            access: required(router, "access")?,
            additional_directories: optional(fields, "additionalDirectories")?.unwrap_or_default(),
            fork,
            root_message_id: optional(router, "rootMessageId")?,
        },
        created_by: created_by.into(),
        approver: approver.into(),
        user_mcp: configuration
            .fingerprint()
            .map_err(|_| InvalidNativeLaunchInput)?,
    })
}

#[cfg(test)]
mod tests {
    use super::native_declared_launch_input;
    use crate::AcpSchemaCatalog;
    use serde_json::json;

    #[test]
    fn native_launch_input_freezes_declared_fields_and_ignores_derived_scratch_metadata() {
        let endpoint =
            json!({"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"});
        let actor = json!({"endpoint":endpoint,"sessionId":"permission-route-owner"});
        let params = json!({"cwd":"/fixture/work","mcpServers":[],"_meta":{"codexRouter":{
            "projectId":"019f0000-0000-7000-8000-000000000004","model":"fixture-model","effort":"low","access":"workspace-write",
            "createdBy":actor,"approver":actor
        }}});
        let endpoint = serde_json::from_value(endpoint).unwrap();
        let mut catalog = AcpSchemaCatalog::load().unwrap();
        let original = native_declared_launch_input(&mut catalog, &params, &endpoint)
            .unwrap()
            .identity()
            .unwrap();
        for (field, value) in [
            ("model", json!("another-model")),
            ("effort", json!("high")),
            ("access", json!("write-restricted")),
            ("forkThreadId", json!("another-thread")),
        ] {
            let mut changed = params.clone();
            changed["_meta"]["codexRouter"][field] = value;
            let identity = native_declared_launch_input(&mut catalog, &changed, &endpoint)
                .unwrap()
                .identity()
                .unwrap();
            assert!(identity != original);
        }
        let mut derived = params;
        derived["_meta"]["codexRouter"]["scratchScope"] = json!("host-derived-scope");
        derived["_meta"]["codexRouter"]["scratchPath"] = json!("/host/derived/path");
        let identity = native_declared_launch_input(&mut catalog, &derived, &endpoint)
            .unwrap()
            .identity()
            .unwrap();
        assert!(identity == original);
    }
}
