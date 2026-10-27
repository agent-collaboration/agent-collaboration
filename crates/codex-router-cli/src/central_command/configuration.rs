//! Central's public deployment identity and its separately protected signing key.
use super::{CentralCommandError, arguments::ServeArguments};
use base64::{Engine, engine::general_purpose::STANDARD};
use codex_router_core::redaction::SecretString;
use codex_router_secret_store::{SecretStore, central_credentials::CentralSecretKind};
use collaboration_central_core::AccessPolicy;
use collaboration_central_tokio::SelfHostedIssuer;
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct CentralConfiguration {
    pub policy: AccessPolicy,
    pub receiver: fleet_sync_protocol::ReceiverId,
    pub relay: fleet_sync_protocol::PeerId,
}

pub(super) fn lock_root(root: &Path) -> Result<std::fs::File, CentralCommandError> {
    if !root.is_absolute() {
        return Err(CentralCommandError::InvalidInput("--root must be absolute"));
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)?;
    let metadata = std::fs::symlink_metadata(root)?;
    if !metadata.is_dir()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(CentralCommandError::InvalidInput(
            "central root must be a private directory owned by this account",
        ));
    }
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(root.join("central.lock"))?;
    fs2::FileExt::try_lock_exclusive(&file)
        .map_err(|_| CentralCommandError::InvalidInput("central root is already in use"))?;
    Ok(file)
}

pub(super) fn initialize(
    arguments: &ServeArguments,
) -> Result<(CentralConfiguration, SelfHostedIssuer), CentralCommandError> {
    let root = &arguments.root;
    if !root.is_absolute() {
        return Err(CentralCommandError::InvalidInput("--root must be absolute"));
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)?;
    let secret_root = root.join("secrets");
    #[cfg(debug_assertions)]
    if arguments.allow_plaintext_file_secrets {
        crate::secret_store_factory::validate_plaintext_debug_path(&secret_root).map_err(|_| {
            CentralCommandError::InvalidInput("plaintext secrets require an isolated debug root")
        })?;
        crate::secret_store_factory::initialize_plaintext_debug_root(&secret_root)?;
    }
    let store = crate::secret_store_factory::open_cli_secret_store(&secret_root)?;
    let path = root.join("central.json");
    let existing = path.try_exists()?;
    let config = if existing {
        let config = read(root)?;
        if arguments
            .public_url
            .as_ref()
            .is_some_and(|url| url.trim_end_matches('/') != config.policy.issuer)
            || arguments
                .owner_identity
                .as_ref()
                .is_some_and(|owner| owner != &config.policy.owner_identity)
        {
            return Err(CentralCommandError::InvalidInput(
                "saved central identity differs; use the original public URL and owner identity",
            ));
        }
        config
    } else {
        let issuer = match &arguments.public_url {
            Some(url) => url.trim_end_matches('/').to_owned(),
            None if !arguments.listen.ip().is_unspecified() && arguments.listen.port() != 0 => {
                format!("http://{}", arguments.listen)
            }
            None => {
                return Err(CentralCommandError::InvalidInput(
                    "--public-url is required for a wildcard bind or port zero",
                ));
            }
        };
        let config = CentralConfiguration {
            policy: AccessPolicy {
                jwks_url: format!("{issuer}/.well-known/jwks.json"),
                issuer,
                session_audience: "codex-router-session".into(),
                admin_audience: "codex-router-admin".into(),
                owner_identity: arguments
                    .owner_identity
                    .clone()
                    .unwrap_or_else(|| "local-owner".into()),
            },
            receiver: fleet_sync_protocol::ReceiverId::new(uuid::Uuid::now_v7()).map_err(|_| {
                CentralCommandError::InvalidInput("central receiver identity generation failed")
            })?,
            relay: fleet_sync_protocol::PeerId::new(uuid::Uuid::now_v7()).map_err(|_| {
                CentralCommandError::InvalidInput("central relay identity generation failed")
            })?,
        };
        config.policy.validate()?;
        let url = reqwest::Url::parse(&config.policy.issuer)
            .map_err(|_| CentralCommandError::InvalidInput("invalid --public-url"))?;
        if url.path() != "/" {
            return Err(CentralCommandError::InvalidInput(
                "--public-url must be an origin without a path",
            ));
        }
        config
    };
    let key = CentralSecretKind::SigningKey.secret_key()?;
    let encoded = match store.read_secret(&key) {
        Ok(value) => value,
        Err(codex_router_secret_store::model::SecretStoreError::Filesystem { source, .. })
            if !existing && source.kind() == std::io::ErrorKind::NotFound =>
        {
            let key_bytes = SelfHostedIssuer::generate_signing_key()?;
            let encoded = SecretString::new(STANDARD.encode(key_bytes));
            store.write_staged(&key, &encoded)?;
            encoded
        }
        Err(error) => return Err(error.into()),
    };
    let issuer = decode_issuer(&encoded, &config)?;
    if !existing {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&serde_json::to_vec_pretty(&config).map_err(|_| {
            CentralCommandError::InvalidInput("central configuration encoding failed")
        })?)?;
        file.sync_all()?;
        std::fs::File::open(root)?.sync_all()?;
    }
    Ok((config, issuer))
}

pub(super) fn read(root: &Path) -> Result<CentralConfiguration, CentralCommandError> {
    if !root.is_absolute() {
        return Err(CentralCommandError::InvalidInput("--root must be absolute"));
    }
    let config: CentralConfiguration =
        serde_json::from_slice(&std::fs::read(root.join("central.json"))?)
            .map_err(|_| CentralCommandError::InvalidInput("invalid central configuration"))?;
    config.policy.validate()?;
    Ok(config)
}
pub(super) fn decode_issuer(
    encoded: &SecretString,
    config: &CentralConfiguration,
) -> Result<SelfHostedIssuer, CentralCommandError> {
    let bytes = STANDARD
        .decode(encoded.expose_secret())
        .map_err(|_| CentralCommandError::InvalidInput("invalid central signing key"))?;
    Ok(SelfHostedIssuer::from_pkcs8(&bytes, config.policy.clone())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    #[allow(clippy::panic_in_result_fn)]
    fn initial_start_creates_valid_fleet_identities_and_reopens_the_same_signing_key()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))?;
        let args = ServeArguments {
            root: directory.path().join("central"),
            listen: "127.0.0.1:29843".parse()?,
            public_url: None,
            owner_identity: None,
            #[cfg(debug_assertions)]
            allow_plaintext_file_secrets: true,
        };
        let lock = lock_root(&args.root)?;
        assert_eq!(
            std::fs::metadata(&args.root)?.permissions().mode() & 0o777,
            0o700
        );
        let (first, signer) = initialize(&args)?;
        let keys = serde_json::to_string(&signer.jwks())?;
        assert!(lock_root(&args.root).is_err());
        drop(lock);
        let _lock = lock_root(&args.root)?;
        let (second, reopened) = initialize(&args)?;
        assert_eq!(first.receiver, second.receiver);
        assert_eq!(first.relay, second.relay);
        assert_eq!(keys, serde_json::to_string(&reopened.jwks())?);
        assert_eq!(first.policy.issuer, "http://127.0.0.1:29843");
        Ok(())
    }
}
