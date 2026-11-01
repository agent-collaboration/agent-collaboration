//! Native per-thread config replaces the declared collaboration entry and keeps other MCP servers.
use collaboration_contracts::HarnessSessionBinding;
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("native MCP server configuration is not a map")]
pub struct InvalidSessionBindingConfig;

pub fn insert_session_binding_config(
    config: &mut Map<String, Value>,
    binding: &HarnessSessionBinding,
) -> Result<(), InvalidSessionBindingConfig> {
    let servers = config
        .entry("mcp_servers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or(InvalidSessionBindingConfig)?;
    servers.insert(
        binding.server_name().to_owned(),
        json!({"url":binding.expose_address()}),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::insert_session_binding_config;
    use collaboration_contracts::HarnessSessionBinding;
    use serde_json::{Map, json};

    #[test]
    fn session_binding_config_replaces_only_the_declared_name_with_each_sessions_full_address() {
        for capability in [
            "11111111111111111111111111111111",
            "22222222222222222222222222222222",
        ] {
            let address = format!("http://127.0.0.1:43210/mcp/session/{capability}");
            let binding = HarnessSessionBinding::from_host_record(
                "agent-router".to_owned().try_into().unwrap(),
                address.clone(),
            );
            let initial = json!({"model_reasoning_effort":"low", "mcp_servers":{
                "agent-router":{"url":"http://127.0.0.1:43210/mcp"},
                "notes":{"command":"/fixture/notes","args":["literal-argument"],"env":{}}
            }});
            let mut config: Map<_, _> = serde_json::from_value(initial.clone()).unwrap();
            insert_session_binding_config(&mut config, &binding).unwrap();
            let mut expected = initial;
            expected["mcp_servers"]["agent-router"] = json!({"url":address});
            assert_eq!(serde_json::to_value(config).unwrap(), expected);
        }
    }

    #[test]
    fn session_binding_config_creates_the_declared_entry_and_refuses_a_malformed_server_map() {
        let binding = HarnessSessionBinding::from_host_record(
            "agent-router".to_owned().try_into().unwrap(),
            "http://127.0.0.1:43210/mcp/session/11111111111111111111111111111111".into(),
        );
        let mut config = Map::new();
        insert_session_binding_config(&mut config, &binding).unwrap();
        assert_eq!(
            serde_json::to_value(config).unwrap(),
            json!({"mcp_servers":{"agent-router":{"url":binding.expose_address()}}})
        );
        for malformed in [json!(null), json!([]), json!("not-a-map")] {
            let mut config = Map::from_iter([("mcp_servers".to_owned(), malformed)]);
            assert!(insert_session_binding_config(&mut config, &binding).is_err());
        }
    }
}
