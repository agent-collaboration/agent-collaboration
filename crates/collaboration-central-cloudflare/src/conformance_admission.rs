//! Owner-authenticated conformance controls on actual admission and runtime state.

use crate::{access_verification, cloudflare_transport};
use collaboration_central_core::AccessCaller;
use serde::Deserialize;
use worker::{Env, Headers, Method, Request, RequestInit, Response, Result};

#[cfg(feature = "conformance-faults")]
#[derive(serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AttachmentFault {
    Missing,
    Invalid,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
enum Command {
    FleetSchema,
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
    ForwardState {
        service: message_board::ServiceId,
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
        service: Option<message_board::ServiceId>,
    },
    AttachmentCodec {
        service: message_board::ServiceId,
        connection: fleet_sync_protocol::ConnectionId,
    },
    #[cfg(feature = "conformance-faults")]
    SessionAttachmentFault {
        service: message_board::ServiceId,
        connection: fleet_sync_protocol::ConnectionId,
        fault: AttachmentFault,
    },
    SessionAlarm {
        service: message_board::ServiceId,
        connection: fleet_sync_protocol::ConnectionId,
    },
    #[cfg(feature = "conformance-faults")]
    SeedSessionAlarm {
        service: message_board::ServiceId,
        now: collaboration_central_core::RegistryTime,
        at: collaboration_central_core::RegistryTime,
    },
    #[cfg(feature = "conformance-faults")]
    SessionWake {
        service: message_board::ServiceId,
        connection: fleet_sync_protocol::ConnectionId,
        at: collaboration_central_core::RegistryTime,
    },
    SessionFault {
        service: message_board::ServiceId,
        command: collaboration_central_conformance::SessionFaultControl,
    },
}

pub(crate) async fn fetch(mut request: Request, env: Env) -> Result<Response> {
    if !owner_allowed(&request, &env).await? {
        return Response::error("owner required", 401);
    }
    let command: Command = request.json().await?;
    forward_command(command, env).await
}

async fn forward_command(command: Command, env: Env) -> Result<Response> {
    let (address, path, body) = match command {
        #[cfg(feature = "conformance-faults")]
        Command::SeedSessionAlarm {
            service: _service,
            now,
            at,
        } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/seed-session-alarm",
            Some(serde_json::to_string(&(now, at))?),
        ),
        #[cfg(feature = "conformance-faults")]
        Command::SessionWake {
            service: _service,
            connection,
            at,
        } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/request-session-wake",
            Some(serde_json::to_string(&(connection, at))?),
        ),
        Command::FleetSchema => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/fleet-schema",
            None,
        ),
        #[cfg(feature = "conformance-faults")]
        Command::SessionBarrier {
            service: _service,
            connection,
        } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/session-finalize-barrier",
            Some(serde_json::to_string(&connection)?),
        ),
        #[cfg(feature = "conformance-faults")]
        Command::SessionJournal {
            service: _service,
            operation,
        } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/session-finalize-journal",
            Some(serde_json::to_string(&operation)?),
        ),
        Command::HolderObservation { service: _service } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/holder-observation",
            None,
        ),
        Command::HolderSessionIdentity {
            service: _service,
            connection,
        } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/holder-session-identity",
            Some(serde_json::to_string(&connection)?),
        ),
        Command::SessionFault {
            service: _service,
            command,
        } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/session-faults",
            Some(serde_json::to_string(&command)?),
        ),
        Command::SessionConnections { service } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/session-connections",
            Some(serde_json::to_string(&service)?),
        ),
        Command::AttachmentCodec {
            service: _service,
            connection,
        } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/session-attachment-codec",
            Some(serde_json::to_string(&connection)?),
        ),
        #[cfg(feature = "conformance-faults")]
        Command::SessionAttachmentFault {
            service: _service,
            connection,
            fault,
        } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/session-attachment-fault",
            Some(serde_json::to_string(&(connection, fault))?),
        ),
        Command::SessionAlarm {
            service: _service,
            connection,
        } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/session-alarm",
            Some(serde_json::to_string(&connection)?),
        ),
        Command::ForwardState { service: _service } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/forward-state",
            None,
        ),
        Command::HolderFrames { service: _service } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/holder-frames",
            None,
        ),
        Command::Origins { service: _service } => (
            crate::cloudflare_transport::legacy_address(&env)?,
            "/test/admission-origins",
            None,
        ),
        Command::SchemaReport { peer } => (
            crate::cloudflare_transport::legacy_address(&env)?,
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

pub(crate) async fn forward_state(mut request: Request, env: Env) -> Result<Response> {
    if !owner_allowed(&request, &env).await? {
        return Response::error("owner required", 401);
    }
    let input: collaboration_central_core::ForwardStateRequest = request.json().await?;
    forward_command(
        Command::ForwardState {
            service: input.service,
        },
        env,
    )
    .await
}

pub(crate) async fn fleet_schema(request: Request, env: Env) -> Result<Response> {
    if !owner_allowed(&request, &env).await? {
        return Response::error("owner required", 401);
    }
    forward_command(Command::FleetSchema, env).await
}

/// Read an organization's stamp and records through its owning Fleet object.
pub(crate) async fn organization_records(mut request: Request, env: Env) -> Result<Response> {
    if !owner_allowed(&request, &env).await? {
        return Response::error("owner required", 401);
    }
    let input: collaboration_central_core::OrganizationRecordsRequest = match request.json().await {
        Ok(input) => input,
        Err(_) => return Response::error("invalid organization records request", 400),
    };
    crate::composed_control::forward(
        &env,
        &collaboration_central_core::ObjectAddress::Fleet {
            organization: input.organization,
        },
        "/test/organization-records",
        None,
    )
    .await
}

pub(crate) async fn fleet_mail_custody(mut request: Request, env: Env) -> Result<Response> {
    if !owner_allowed(&request, &env).await? {
        return Response::error("owner required", 401);
    }
    let input: collaboration_central_core::FleetMailCustodyRequest = match request.json().await {
        Ok(input) => input,
        Err(_) => return Response::error("invalid custody item request", 400),
    };
    crate::composed_control::forward(
        &env,
        &crate::cloudflare_transport::legacy_address(&env)?,
        "/test/fleet-mail-custody",
        Some(serde_json::to_string(&input)?),
    )
    .await
}

async fn owner_allowed(request: &Request, env: &Env) -> Result<bool> {
    let labels = access_verification::InvocationLabels::for_request(request, env)?;
    let assertion = request.headers().get("Cf-Access-Jwt-Assertion")?;
    match access_verification::verify(
        env,
        assertion.as_deref(),
        "/admin/v1/conformance",
        &labels,
        access_verification::VerificationLayer::Edge,
    )
    .await
    {
        Ok(AccessCaller::Owner { .. }) => Ok(true),
        _ => Ok(false),
    }
}
