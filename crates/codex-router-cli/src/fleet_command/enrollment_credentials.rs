//! Each source determines its own header and secret representation. No fallback.
use super::{
    FleetCommandError,
    arguments::EnrollArguments,
    enrollment_prepare::{PreparedAccess, read_access_secret, sensitive_header},
};
use crate::CliContext;
use chrono::{DateTime, Utc};
use codex_router_core::redaction::SecretString;
use codex_router_secret_store::{
    SecretStore,
    central_credentials::{CentralCredential, CentralSecretKind},
};
use collaboration_central_core::AccessClientId;
use reqwest::{Url, header::HeaderValue};

pub(super) struct EnrollmentCredentials {
    pub client_id: AccessClientId,
    pub owner_token: HeaderValue,
    pub owner_header: &'static str,
    pub access: PreparedAccess,
    pub expires_at: DateTime<Utc>,
}

pub(super) fn resolve(
    args: &EnrollArguments,
    context: &CliContext,
    admin_url: &Url,
) -> Result<EnrollmentCredentials, FleetCommandError> {
    let handoff = if let Some(root) = &args.self_hosted_root {
        if !root.is_absolute() {
            return Err(FleetCommandError::InvalidInput(
                "--self-hosted-root must be absolute",
            ));
        }
        let store = codex_router_host::open_fleet_enrollment_store(root)?;
        let encoded = store.read_secret(&CentralSecretKind::EnrollmentCredential.secret_key()?)?;
        Some(CentralCredential::decode_secret(&encoded)?)
    } else if let Some(file) = &args.enrollment_credential_file {
        Some(CentralCredential::consume_private_file(file)?)
    } else {
        None
    };
    if let Some(handoff) = handoff {
        let CentralCredential::Enrollment {
            endpoint,
            owner_assertion,
            owner_expires_at,
            client_id,
            assertion,
            expires_at,
        } = handoff
        else {
            return Err(FleetCommandError::InvalidInput(
                "expected a machine enrollment credential",
            ));
        };
        let origin = Url::parse(&endpoint)
            .map_err(|_| FleetCommandError::InvalidInput("invalid credential endpoint"))?;
        if origin.origin() != admin_url.origin() {
            return Err(FleetCommandError::InvalidInput(
                "credential belongs to a different central endpoint",
            ));
        }
        future_expiry(&owner_expires_at)?;
        sensitive_header(&assertion)?;
        return Ok(EnrollmentCredentials {
            client_id: AccessClientId::new(client_id)
                .map_err(|_| FleetCommandError::InvalidInput("invalid central client identity"))?,
            owner_token: sensitive_header(&owner_assertion)?,
            owner_header: "Cf-Access-Jwt-Assertion",
            access: PreparedAccess::SelfHosted(SecretString::new(assertion)),
            expires_at: future_expiry(&expires_at)?,
        });
    }
    let expires_at = DateTime::parse_from_rfc3339(
        args.expires_at
            .as_deref()
            .ok_or(FleetCommandError::InvalidInput("--expires-at is required"))?,
    )
    .map_err(|_| {
        FleetCommandError::InvalidInput(
            "--expires-at must be an RFC3339 timestamp matching the Access service token expiry",
        )
    })?
    .with_timezone(&Utc);
    if expires_at <= Utc::now() {
        return Err(FleetCommandError::InvalidInput(
            "--expires-at must be in the future",
        ));
    }
    if args.owner_token_env.is_empty()
        || args.owner_token_env.bytes().any(|b| matches!(b, 0 | b'='))
    {
        return Err(FleetCommandError::InvalidInput(
            "--owner-token-env must name an environment variable",
        ));
    }
    let owner = context
        .env_var(&args.owner_token_env)
        .ok_or(FleetCommandError::InvalidInput(
            "the selected owner-token environment variable is unavailable",
        ))?;
    Ok(EnrollmentCredentials {
        client_id: AccessClientId::new(args.access_client_id.clone().ok_or(
            FleetCommandError::InvalidInput("--access-client-id is required"),
        )?)
        .map_err(|_| FleetCommandError::InvalidInput("invalid Access client ID"))?,
        owner_token: sensitive_header(owner)?,
        owner_header: "cf-access-token",
        access: PreparedAccess::Cloudflare(read_access_secret(
            args.access_client_secret_file
                .as_deref()
                .ok_or(FleetCommandError::InvalidInput(
                    "--access-client-secret-file is required",
                ))?,
        )?),
        expires_at,
    })
}

fn future_expiry(value: &str) -> Result<DateTime<Utc>, FleetCommandError> {
    let expiry = DateTime::parse_from_rfc3339(value)
        .map_err(|_| {
            FleetCommandError::InvalidInput("credential expiry must be an RFC3339 timestamp")
        })?
        .with_timezone(&Utc);
    if expiry <= Utc::now() {
        return Err(FleetCommandError::InvalidInput(
            "credential has expired; mint a new credential and enroll again",
        ));
    }
    Ok(expiry)
}
