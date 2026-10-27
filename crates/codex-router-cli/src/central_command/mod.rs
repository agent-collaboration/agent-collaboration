mod arguments;
mod configuration;
#[cfg(test)]
mod credential_lifetimes_tests;
pub(crate) use arguments::CentralCommand;
use arguments::{CredentialKind, MintArguments};
use chrono::{Duration as ChronoDuration, SubsecRound, Utc};
use codex_router_secret_store::{
    SecretStore,
    central_credentials::{CentralCredential, CentralSecretKind},
};
use collaboration_central_core::{AccessCaller, AccessClientId};
use collaboration_central_tokio::{TokioServer, TokioSettings};
use std::{io::Write, time::Duration};

#[derive(Debug, thiserror::Error)]
pub enum CentralCommandError {
    #[error("{0}")]
    Arguments(String),
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error("central local operation failed")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    SecretStore(#[from] codex_router_secret_store::model::SecretStoreError),
    #[error(transparent)]
    CredentialFile(#[from] codex_router_secret_store::central_credentials::CredentialFileError),
    #[error(transparent)]
    Issuer(#[from] collaboration_central_tokio::IssuerError),
    #[error(transparent)]
    Policy(#[from] collaboration_central_core::AccessRefusal),
    #[error(transparent)]
    Server(#[from] collaboration_central_tokio::TokioServerError),
    #[error("central local task failed")]
    LocalTask,
    #[error("central commands require async dispatch")]
    AsyncDispatchRequired,
}

pub(crate) async fn run_central_command(
    command: CentralCommand,
    stdout: &mut (impl Write + Send),
) -> Result<(), CentralCommandError> {
    match command {
        CentralCommand::Help(help) => stdout.write_all(help.as_bytes())?,
        CentralCommand::Mint(arguments) => {
            let expiry = tokio::task::spawn_blocking(move || mint(arguments))
                .await
                .map_err(|_| CentralCommandError::LocalTask)??;
            writeln!(stdout, "credential ready; expires at {expiry}")?;
        }
        CentralCommand::Serve(arguments) => {
            let listen = arguments.listen;
            let root = arguments.root.clone();
            let (config, issuer, _root_lock) = tokio::task::spawn_blocking(move || {
                let root_lock = configuration::lock_root(&arguments.root)?;
                let (config, issuer) = configuration::initialize(&arguments)?;
                Ok::<_, CentralCommandError>((config, issuer, root_lock))
            })
            .await
            .map_err(|_| CentralCommandError::LocalTask)??;
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            let server = TokioServer::start_with_jwks(
                TokioSettings {
                    root: root.join("objects"),
                    jwks_url: config.policy.jwks_url.clone(),
                    access_policy: config.policy,
                    registry: fleet_sync_lease::RegistryConfig {
                        lease: fleet_sync_lease::LeasePolicy {
                            lease_duration: Duration::from_secs(14400),
                            renew_interval: Duration::from_secs(900),
                        },
                        authoritative_receiver: config.receiver,
                        release_unsent_for: Duration::from_secs(300),
                        move_needs_renewal_within: Some(Duration::from_secs(1800)),
                    },
                    delivery: collaboration_central_tokio::DeliveryConfig {
                        relay_peer: config.relay,
                    },
                    holder_answer_timeout: Duration::from_secs(30),
                    copy_ack_wait: Duration::from_secs(30),
                },
                listen,
                issuer.jwks(),
            )
            .await?;
            writeln!(stdout, "central listening: {}", server.base_url())?;
            stdout.flush()?;
            tokio::select! { result = tokio::signal::ctrl_c() => { result?; }, _ = terminate.recv() => {} }
            server.shutdown().await?;
            writeln!(stdout, "central stopped")?;
        }
    }
    Ok(())
}

fn mint(arguments: MintArguments) -> Result<String, CentralCommandError> {
    let config = configuration::read(&arguments.root)?;
    let store =
        crate::secret_store_factory::open_cli_secret_store(&arguments.root.join("secrets"))?;
    let key = store.read_secret(&CentralSecretKind::SigningKey.secret_key()?)?;
    let issuer = configuration::decode_issuer(&key, &config)?;
    let now = Utc::now().trunc_subsecs(0);
    let owner_expiry = now + ChronoDuration::minutes(15);
    let owner_assertion = issuer.mint(
        &AccessCaller::Owner {
            email: config.policy.owner_identity,
        },
        now,
        owner_expiry,
    )?;
    let endpoint = config.policy.issuer;
    let (kind, credential, expiry) = match arguments.kind {
        CredentialKind::Owner => (
            CentralSecretKind::OwnerCredential,
            CentralCredential::Owner {
                endpoint,
                assertion: owner_assertion,
                expires_at: owner_expiry.to_rfc3339(),
            },
            owner_expiry,
        ),
        CredentialKind::Enrollment => {
            let client_id =
                AccessClientId::new(String::from(collaboration_runtime::new_service_uuid()?))?;
            let expiry = now + ChronoDuration::days(i64::from(arguments.machine_valid_for_days));
            let assertion = issuer.mint(
                &AccessCaller::Machine {
                    client_id: client_id.clone(),
                },
                now,
                expiry,
            )?;
            (
                CentralSecretKind::EnrollmentCredential,
                CentralCredential::Enrollment {
                    endpoint,
                    owner_assertion,
                    owner_expires_at: owner_expiry.to_rfc3339(),
                    client_id: client_id.as_str().into(),
                    assertion,
                    expires_at: expiry.to_rfc3339(),
                },
                expiry,
            )
        }
    };
    if let Some(path) = arguments.out {
        credential.write_private_file(&path)?;
    } else {
        store.write_staged(&kind.secret_key()?, &credential.encode_secret()?)?;
    }
    Ok(expiry.to_rfc3339())
}
