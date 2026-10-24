//! Owner-authenticated conformance controls on actual admission and runtime state.

use crate::{access_verification, cloudflare_transport};
use collaboration_central_core::{AccessCaller, ObjectAddress};
use serde::Deserialize;
use worker::*;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum Command {
    Origins {
        service: message_board::ServiceId,
    },
    SchemaReport {
        peer: fleet_sync_protocol::PeerId,
    },
    HolderObservation {
        service: message_board::ServiceId,
    },
    HolderSessionIdentity {
        service: message_board::ServiceId,
        connection: fleet_sync_protocol::ConnectionId,
    },
    HolderFrames {
        service: message_board::ServiceId,
    },
    #[cfg(feature = "conformance-faults")]
    SessionBarrier {
        service: message_board::ServiceId,
        connection: fleet_sync_protocol::ConnectionId,
    },
    #[cfg(feature = "conformance-faults")]
    SessionJournal {
        service: message_board::ServiceId,
        operation: durable_steps::OperationId,
    },
    SessionConnections {
        service: message_board::ServiceId,
    },
    AttachmentCodec {
        service: message_board::ServiceId,
        connection: fleet_sync_protocol::ConnectionId,
    },
    SessionAlarm {
        service: message_board::ServiceId,
        connection: fleet_sync_protocol::ConnectionId,
    },
    SessionFault {
        service: message_board::ServiceId,
        command: collaboration_central_conformance::SessionFaultControl,
    },
}

pub(crate) async fn fetch(mut request: Request, env: Env) -> Result<Response> {
    let labels = access_verification::InvocationLabels::for_request(&request)?;
    let assertion = request.headers().get("Cf-Access-Jwt-Assertion")?;
    match access_verification::verify(
        &env,
        assertion.as_deref(),
        "/admin/v1/conformance",
        &labels,
        access_verification::VerificationLayer::Edge,
    )
    .await
    {
        Ok(AccessCaller::Owner { .. }) => {}
        _ => return Response::error("owner required", 401),
    }
    let command: Command = request.json().await?;
    let (address, path, body) = match command {
        #[cfg(feature = "conformance-faults")]
        Command::SessionBarrier {
            service,
            connection,
        } => (
            ObjectAddress::Connection(service),
            "/test/session-finalize-barrier",
            Some(serde_json::to_string(&connection)?),
        ),
        #[cfg(feature = "conformance-faults")]
        Command::SessionJournal { service, operation } => (
            ObjectAddress::Connection(service),
            "/test/session-finalize-journal",
            Some(serde_json::to_string(&operation)?),
        ),
        Command::HolderObservation { service } => (
            ObjectAddress::Connection(service),
            "/test/holder-observation",
            None,
        ),
        Command::HolderSessionIdentity {
            service,
            connection,
        } => (
            ObjectAddress::Connection(service),
            "/test/holder-session-identity",
            Some(serde_json::to_string(&connection)?),
        ),
        Command::SessionFault { service, command } => (
            ObjectAddress::Connection(service),
            "/test/session-faults",
            Some(serde_json::to_string(&command)?),
        ),
        Command::SessionConnections { service } => (
            ObjectAddress::Connection(service),
            "/test/session-connections",
            None,
        ),
        Command::AttachmentCodec {
            service,
            connection,
        } => (
            ObjectAddress::Connection(service),
            "/test/session-attachment-codec",
            Some(serde_json::to_string(&connection)?),
        ),
        Command::SessionAlarm {
            service,
            connection,
        } => (
            ObjectAddress::Connection(service),
            "/test/session-alarm",
            Some(serde_json::to_string(&connection)?),
        ),
        Command::HolderFrames { service } => (
            ObjectAddress::Connection(service),
            "/test/holder-frames",
            None,
        ),
        Command::Origins { service } => (
            ObjectAddress::Connection(service),
            "/test/admission-origins",
            None,
        ),
        Command::SchemaReport { peer } => (
            ObjectAddress::Registry,
            "/test/schema-report",
            Some(serde_json::to_string(&peer)?),
        ),
    };
    let headers = Headers::new();
    cloudflare_transport::address_header(&headers, &address)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(body.map(Into::into));
    cloudflare_transport::stub(&env, &address)?
        .fetch_with_request(Request::new_with_init(
            &format!("https://central.internal{path}"),
            &init,
        )?)
        .await
}
