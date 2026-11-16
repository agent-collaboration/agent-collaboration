//! `cargo run -p codex-router-test-support --example export_acp_schema -- <out.json>`
//!
//! Writes the ACP true definition (D0 R50, Y1): the JSON Schema of the ACP library the Router
//! compiles, `agent-client-protocol` 2.2.0 with `agent-client-protocol-schema` 1.9.1, built with
//! the same features the Router builds it with (none of the `unstable_*` features).
//!
//! The root type and the schemars settings are upstream's own generator, copied from
//! `schema-generator/src/main.rs` at the schema crate's release commit
//! `7e87dc205a7325bd07d0249fd20bb7486ee6ba95` (the `.cargo_vcs_info.json` of 1.9.1), in its stable
//! v1 configuration: `AcpTypes` and `root_schema_value()`, with no publication rewrite (upstream
//! rewrites only under `unstable_protocol_v2`). The output is written exactly as upstream writes
//! `schema/v1/schema.json`: pretty JSON, no trailing newline.

use agent_client_protocol::schema::v1::{
    AgentNotification, AgentRequest, AgentResponse, ClientNotification, ClientRequest,
    ClientResponse, JsonRpcMessage, Notification, ProtocolLevelNotification, Request, Response,
};
use schemars::{
    JsonSchema,
    generate::SchemaSettings,
    transform::{RemoveRefSiblings, ReplaceBoolSchemas},
};

/// All messages that an agent can send to a client.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(inline)]
#[expect(dead_code, reason = "Only the schema of the type is generated.")]
#[expect(
    clippy::large_enum_variant,
    reason = "Upstream's generator type, copied unchanged (it allows this lint); never instantiated."
)]
enum AgentOutgoingMessage {
    Request(Request<AgentRequest>),
    Response(Response<AgentResponse>),
    Notification(Notification<AgentNotification>),
}

/// All messages that a client can send to an agent.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(inline)]
#[expect(dead_code, reason = "Only the schema of the type is generated.")]
#[expect(
    clippy::large_enum_variant,
    reason = "Upstream's generator type, copied unchanged (it allows this lint); never instantiated."
)]
enum ClientOutgoingMessage {
    Request(Request<ClientRequest>),
    Response(Response<ClientResponse>),
    Notification(Notification<ClientNotification>),
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(title = "Agent Client Protocol")]
#[expect(dead_code, reason = "Only the schema of the type is generated.")]
enum AcpTypes {
    Agent(JsonRpcMessage<AgentOutgoingMessage>),
    Client(JsonRpcMessage<ClientOutgoingMessage>),
    ProtocolLevel(JsonRpcMessage<Notification<ProtocolLevelNotification>>),
}

fn root_schema_value() -> Result<serde_json::Value, serde_json::Error> {
    let mut settings = SchemaSettings::draft2020_12();
    settings.untagged_enum_variant_titles = true;
    let mut bool_schemas = ReplaceBoolSchemas::default();
    bool_schemas.skip_additional_properties = true;
    settings = settings
        .with_transform(RemoveRefSiblings::default())
        .with_transform(bool_schemas);
    let schema = settings.into_generator().into_root_schema_for::<AcpTypes>();
    serde_json::to_value(&schema)
}

fn main() -> std::process::ExitCode {
    let Some(out) = std::env::args_os().nth(1) else {
        eprintln!("usage: export_acp_schema <out.json>");
        return std::process::ExitCode::from(2);
    };
    let written = root_schema_value()
        .and_then(|schema| serde_json::to_string_pretty(&schema))
        .map_err(|error| error.to_string())
        .and_then(|text| std::fs::write(&out, text).map_err(|error| error.to_string()));
    match written {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("export_acp_schema: {problem}");
            std::process::ExitCode::FAILURE
        }
    }
}
