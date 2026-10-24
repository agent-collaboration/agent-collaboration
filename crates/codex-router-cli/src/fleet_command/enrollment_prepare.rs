//! Validate local inputs and retain the generated credential only in memory until admission.
use super::{FleetCommandError, arguments::EnrollArguments};
use crate::CliContext;
use chrono::{DateTime, Utc};
use codex_router_core::redaction::SecretString;
use collaboration_central_core::{AccessClientId, EnrollMachine};
use reqwest::{Url, header::HeaderValue};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    io::Read,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub(super) struct PreparedEnrollment {
    pub root: PathBuf,
    pub endpoint: String,
    pub admin_url: Url,
    pub request: EnrollMachine,
    pub owner_token: HeaderValue,
    pub access_secret: SecretString,
    pub machine_credential: SecretString,
    #[cfg(debug_assertions)]
    pub allow_plaintext_file_secrets: bool,
}

pub(super) fn prepare(
    arguments: EnrollArguments,
    context: &CliContext,
) -> Result<PreparedEnrollment, FleetCommandError> {
    let expires_at = DateTime::parse_from_rfc3339(&arguments.expires_at)
        .map_err(|_| FleetCommandError::InvalidInput("--expires-at must be an RFC3339 timestamp matching the Access service token expiry"))?.with_timezone(&Utc);
    if expires_at <= Utc::now() {
        return Err(FleetCommandError::InvalidInput(
            "--expires-at must be in the future",
        ));
    }
    if arguments.owner_token_env.is_empty()
        || arguments
            .owner_token_env
            .bytes()
            .any(|byte| matches!(byte, 0 | b'='))
    {
        return Err(FleetCommandError::InvalidInput(
            "--owner-token-env must name an environment variable",
        ));
    }
    let owner =
        context
            .env_var(&arguments.owner_token_env)
            .ok_or(FleetCommandError::InvalidInput(
                "the selected owner-token environment variable is unavailable",
            ))?;
    let owner_token = sensitive_header(owner)?;
    let endpoint_url = Url::parse(&arguments.endpoint)
        .map_err(|_| FleetCommandError::InvalidInput("invalid central session endpoint"))?;
    if !matches!(endpoint_url.scheme(), "ws" | "wss" | "http" | "https")
        || endpoint_url.host_str().is_none()
        || !endpoint_url.username().is_empty()
        || endpoint_url.password().is_some()
        || endpoint_url.query().is_some()
        || endpoint_url.fragment().is_some()
    {
        return Err(FleetCommandError::InvalidInput(
            "central session endpoint requires ws, wss, http or https without user info, query or fragment",
        ));
    }
    let mut admin_url = endpoint_url.clone();
    let scheme = if matches!(admin_url.scheme(), "wss" | "https") {
        "https"
    } else {
        "http"
    };
    admin_url
        .set_scheme(scheme)
        .map_err(|()| FleetCommandError::InvalidInput("invalid central endpoint scheme"))?;
    admin_url.set_path("/admin/v1/machines/enroll");
    let access_client_id = AccessClientId::new(arguments.access_client_id)
        .map_err(|_| FleetCommandError::InvalidInput("invalid Access client ID"))?;
    let label = arguments
        .label
        .try_into()
        .map_err(|_| FleetCommandError::InvalidInput("invalid machine label"))?;
    let access_secret = read_access_secret(&arguments.access_client_secret_file)?;
    let root = crate::router_root_or_default(arguments.router_root).map_err(|_| {
        FleetCommandError::InvalidInput("Router root is unavailable; pass --router-root")
    })?;
    if !root.is_absolute() {
        return Err(FleetCommandError::InvalidInput(
            "--router-root must be absolute",
        ));
    }
    #[cfg(debug_assertions)]
    if arguments.allow_plaintext_file_secrets {
        let secret_root = codex_router_host::HostCoordinationPaths::for_router_root(&root)
            .router_secret_root()
            .ok_or(FleetCommandError::InvalidInput(
                "Router secret root is unavailable",
            ))?;
        crate::secret_store_factory::validate_plaintext_debug_path(&secret_root).map_err(|_| {
            FleetCommandError::InvalidInput("plaintext secrets require an isolated debug root")
        })?;
    }
    let directory = codex_router_host::router_collaboration_directory(&root);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)?;
    let installation = String::from(collaboration_service::load_service_identity(&directory)?)
        .try_into()
        .map_err(|_| FleetCommandError::InvalidInput("invalid installation identity"))?;
    let peer = collaboration_service::load_installation_peer(&directory)?;
    let raw = random_machine_token(&mut std::fs::File::open("/dev/urandom")?)?;
    let credential_hash = raw_token_hash(&raw);
    Ok(PreparedEnrollment {
        root,
        endpoint: endpoint_url.to_string(),
        admin_url,
        request: EnrollMachine {
            installation,
            peer,
            label,
            access_client_id,
            credential_hash,
            expires_at,
        },
        owner_token,
        access_secret,
        machine_credential: SecretString::new(format!("Bearer {raw}")),
        #[cfg(debug_assertions)]
        allow_plaintext_file_secrets: arguments.allow_plaintext_file_secrets,
    })
}

pub(super) fn sensitive_header(value: &str) -> Result<HeaderValue, FleetCommandError> {
    if value.is_empty() || value.bytes().any(|byte| matches!(byte, b'\r' | b'\n')) {
        return Err(FleetCommandError::InvalidInput(
            "credential is unavailable or malformed",
        ));
    }
    let mut header = HeaderValue::from_str(value)
        .map_err(|_| FleetCommandError::InvalidInput("credential is unavailable or malformed"))?;
    header.set_sensitive(true);
    Ok(header)
}

fn read_access_secret(path: &Path) -> Result<SecretString, FleetCommandError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !path.is_absolute()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != nix::unistd::Uid::effective().as_raw()
        || metadata.len() > 32768
    {
        return Err(FleetCommandError::InvalidInput(
            "Access client secret requires a bounded private owner-only regular file",
        ));
    }
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| FleetCommandError::InvalidInput("Access client secret is malformed"))?;
    let secret = text.trim_end_matches(['\r', '\n']);
    sensitive_header(secret)?;
    Ok(SecretString::new(secret.to_owned()))
}

pub(super) fn raw_token_hash(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

pub(super) fn random_machine_token(reader: &mut impl Read) -> Result<String, FleetCommandError> {
    let mut bytes = [0_u8; 32];
    reader.read_exact(&mut bytes)?;
    let mut token = String::with_capacity(64);
    for byte in bytes {
        write!(&mut token, "{byte:02x}")
            .map_err(|_| FleetCommandError::InvalidInput("machine credential encoding failed"))?;
    }
    Ok(token)
}
