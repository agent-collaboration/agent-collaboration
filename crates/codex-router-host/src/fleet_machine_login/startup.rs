//! Selects machine-token mode from explicit deployment input, once per Host lifetime.
use std::{io, path::Path, sync::Arc};

use board_fleet_sync::{AccessServiceToken, BoardSessionInputs};
use codex_router_secret_store::{
    SecretStore,
    classified_store_open::ClassifiedCredentialStore,
    fleet_secret_key::{FleetSecretKey, FleetSecretKind},
    keychain_data_key::KeychainAccess,
};
use fleet_sync_protocol::{PeerId, ReceiverId};
use fleet_sync_session::{Endpoint, PeerCredential};

use super::{
    issuer_http::HttpMachineTokenIssuer,
    login_clock::{FleetLoginClock, HostFleetLoginClock},
    login_files::{
        FleetLoginConfiguration, FleetSecretReference, LoginAccessReference, read_configuration,
        read_metadata,
    },
    login_owner::{MachineLoginHandle, MachineLoginIdentity, RunningMachineLogin},
    login_publication::{MachineLoginPublication, sync_directory},
    machine_token_transport::MachineTokenSource,
    refresh_credential_store::RefreshCredentialStore,
};

/// Explicit test capabilities; they never redirect Codex home or Router state.
pub struct FleetLoginDependencies {
    pub clock: Arc<dyn FleetLoginClock>,
    pub keychain: Arc<dyn KeychainAccess>,
}

#[derive(Clone)]
pub(crate) enum MachineLoginBinding {
    NotConfigured { source: MachineTokenSource },
    Configured { handle: MachineLoginHandle },
}
impl MachineLoginBinding {
    pub(crate) fn source(&self) -> MachineTokenSource {
        match self {
            Self::NotConfigured { source } => source.clone(),
            Self::Configured { handle } => handle.source.clone(),
        }
    }
    pub(crate) fn handle(&self) -> Option<MachineLoginHandle> {
        match self {
            Self::NotConfigured { .. } => None,
            Self::Configured { handle } => Some(handle.clone()),
        }
    }
}

pub(crate) enum HostMachineLogin {
    NotConfigured {
        source: MachineTokenSource,
    },
    Configured {
        owner: RunningMachineLogin,
        connection: Option<Box<MachineConnectionInputs>>,
    },
}
pub(crate) struct MachineConnectionInputs {
    endpoint: Endpoint,
    access: AccessServiceToken,
    peer: PeerId,
    receiver: ReceiverId,
    relay: PeerId,
}
impl HostMachineLogin {
    pub(crate) fn binding(&self) -> MachineLoginBinding {
        match self {
            Self::NotConfigured { source } => MachineLoginBinding::NotConfigured {
                source: source.clone(),
            },
            Self::Configured { owner, .. } => MachineLoginBinding::Configured {
                handle: owner.handle.clone(),
            },
        }
    }
    pub(crate) fn session_inputs(&self) -> io::Result<Option<BoardSessionInputs>> {
        let Self::Configured {
            connection: Some(connection),
            ..
        } = self
        else {
            return Ok(None);
        };
        Ok(Some(BoardSessionInputs {
            endpoint: connection.endpoint.clone(),
            access: connection.access.clone(),
            // The stock pump requires a fixed credential argument. DeviceCode
            // transport ignores it and waits for the Host source on each connect.
            credential: PeerCredential::new("host-token-source").map_err(io::Error::other)?,
            peer: connection.peer,
            receiver: connection.receiver,
            relay: connection.relay,
        }))
    }
    pub(crate) async fn shutdown(self) -> io::Result<()> {
        match self {
            Self::Configured { owner, .. } => owner.shutdown().await,
            Self::NotConfigured { .. } => Ok(()),
        }
    }
}

pub(crate) fn start_machine_login(
    router_root: &Path,
    secret_root: &Path,
    directory: &Path,
    dependencies: Option<FleetLoginDependencies>,
) -> io::Result<Option<HostMachineLogin>> {
    let configuration = read_configuration(router_root).map_err(io::Error::other)?;
    let saved = read_metadata(router_root).map_err(io::Error::other)?;
    let clock: Arc<dyn FleetLoginClock> = dependencies.as_ref().map_or_else(
        || Arc::new(HostFleetLoginClock) as Arc<dyn FleetLoginClock>,
        |dependencies| dependencies.clock.clone(),
    );
    let Some(configuration) = configuration else {
        // Removing deployment input must never reactivate a retired step-one
        // enrollment. Public new-login state keeps the board gate closed.
        return Ok(saved.map(|_| {
            let (_, source) = MachineTokenSource::channel(clock);
            HostMachineLogin::NotConfigured { source }
        }));
    };
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let installation: fleet_identity_protocol::InstallationId =
        String::from(collaboration_runtime::load_service_identity(directory)?)
            .try_into()
            .map_err(|_| io::Error::other("invalid saved installation identity"))?;
    let peer = collaboration_runtime::load_installation_peer(directory)?;
    let classified = match dependencies {
        Some(dependencies) => ClassifiedCredentialStore::open_read_only_with_keychain(
            secret_root,
            dependencies.keychain.as_ref(),
        ),
        None => ClassifiedCredentialStore::open_read_only_for_process(secret_root),
    };
    let credentials = Arc::new(RefreshCredentialStore::from_classification(
        classified,
        installation.clone(),
    ));
    let connection = match credentials.as_ref() {
        RefreshCredentialStore::Unusable { .. } => None,
        _ => Some(Box::new(connection_inputs(
            &configuration,
            credentials.as_ref(),
            peer,
        )?)),
    };
    let issuer = Arc::new(
        HttpMachineTokenIssuer::new(
            configuration.device_authorization_url,
            configuration.token_url,
            configuration.device_client_id,
            configuration.machine_resource,
            clock.clone(),
        )
        .map_err(|_| io::Error::other("machine login HTTP client is unavailable"))?,
    );
    let publication = Arc::new(MachineLoginPublication {
        root: router_root.to_owned(),
        credentials,
        sync_directory,
    });
    let owner = RunningMachineLogin::start(
        MachineLoginIdentity { installation, peer },
        issuer,
        clock,
        publication,
        saved,
    );
    Ok(Some(HostMachineLogin::Configured { owner, connection }))
}

fn connection_inputs(
    configuration: &FleetLoginConfiguration,
    credentials: &RefreshCredentialStore,
    peer: PeerId,
) -> io::Result<MachineConnectionInputs> {
    let access = match &configuration.access {
        LoginAccessReference::None {} => AccessServiceToken::NoEdgeCredentials,
        LoginAccessReference::Cloudflare {
            client_id,
            client_secret_key,
        } => {
            let secret = read_edge_secret(
                credentials,
                client_secret_key,
                FleetSecretKind::AccessClientSecret,
            )?;
            AccessServiceToken::new(client_id.as_str(), secret.expose_secret())
                .map_err(io::Error::other)?
        }
        LoginAccessReference::SelfHosted { assertion_key } => {
            let secret = read_edge_secret(
                credentials,
                assertion_key,
                FleetSecretKind::MachineAssertion,
            )?;
            AccessServiceToken::self_hosted(secret.expose_secret()).map_err(io::Error::other)?
        }
    };
    Ok(MachineConnectionInputs {
        endpoint: Endpoint::new(configuration.endpoint.as_str()).map_err(io::Error::other)?,
        access,
        peer,
        receiver: configuration.receiver,
        relay: configuration.relay,
    })
}

fn read_edge_secret(
    credentials: &RefreshCredentialStore,
    reference: &FleetSecretReference,
    expected_kind: FleetSecretKind,
) -> io::Result<codex_router_core::redaction::SecretString> {
    let invalid = || {
        io::Error::other(
            "machine login requires a readable Access secret of the configured purpose and installation",
        )
    };
    let RefreshCredentialStore::Encrypted {
        store,
        installation,
    } = credentials
    else {
        return Err(invalid());
    };
    let key = reference.secret_key().map_err(|_| invalid())?;
    let identity = FleetSecretKey::parse(&key)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if identity.kind() != expected_kind || identity.installation() != installation.as_str() {
        return Err(invalid());
    }
    store.read_secret(&key).map_err(|_| invalid())
}
