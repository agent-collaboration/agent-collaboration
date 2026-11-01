//! Startup selects lifetime keeping and reads the public saved route, never a login JSON file.
use std::{io, path::Path, sync::Arc, time::Duration};

use board_fleet_sync::{AccessServiceToken, BoardSessionInputs};
use codex_router_fleet_state::fleet_login_repository::{
    FleetLoginRepository, FleetLoginStorageFailure,
};
use codex_router_secret_store::{
    SecretStore,
    classified_store_open::ClassifiedCredentialStore,
    fleet_secret_key::{FleetSecretKey, FleetSecretKind},
    keychain_data_key::KeychainAccess,
    model::SecretKey,
};
use fleet_sync_protocol::PeerId;
use fleet_sync_session::{Endpoint, PeerCredential};

use super::{
    issuer_connection::HttpMachineIssuerConnection,
    login_clock::{FleetLoginClock, HostFleetLoginClock},
    login_owner::{
        MachineLoginHandle, MachineLoginIdentity, MachineLoginRoute, RunningMachineLogin,
    },
    login_publication::MachineLoginPublication,
    login_work::MachineLoginWork,
    machine_token_transport::MachineTokenSource,
    refresh_credential_store::RefreshCredentialStore,
};

/// Explicit fixture capabilities; production supplies its own secure HTTP client.
pub struct FleetLoginDependencies {
    pub clock: Arc<dyn FleetLoginClock>,
    pub keychain: Arc<dyn KeychainAccess>,
    pub issuer_client: reqwest::Client,
}

/// Selected by the deployment entrypoint, independently of saved-row presence.
pub(crate) enum MachineLoginStartup {
    AccessEnrollment,
    // Real deployment composition is held on the Fleet backend. The existing
    // isolated entrypoint is the only current caller of token-mode startup.
    #[cfg(any(test, feature = "runtime-test-support"))]
    MachineTokens {
        repository: Option<Arc<dyn FleetLoginRepository>>,
        dependencies: Option<FleetLoginDependencies>,
    },
}

pub(crate) struct MachineTokenStartupInputs {
    pub repository: Arc<dyn FleetLoginRepository>,
    pub dependencies: Option<FleetLoginDependencies>,
}

impl MachineLoginStartup {
    pub(crate) fn resolve(
        self,
    ) -> Result<Option<MachineTokenStartupInputs>, FleetLoginStorageFailure> {
        match self {
            Self::AccessEnrollment => Ok(None),
            #[cfg(any(test, feature = "runtime-test-support"))]
            Self::MachineTokens {
                repository,
                dependencies,
            } => {
                let repository = repository.ok_or(FleetLoginStorageFailure::Unavailable)?;
                Ok(Some(MachineTokenStartupInputs {
                    repository,
                    dependencies,
                }))
            }
        }
    }
}

/// The existing enrollment-file owner supplies only its edge reference.
pub(crate) enum MachineEdgeAccess {
    Unconfigured,
    Cloudflare {
        client_id: String,
        secret_key: SecretKey,
    },
    SelfHosted {
        assertion_key: SecretKey,
    },
}
enum EdgeCredential {
    Ready(AccessServiceToken),
    Unavailable,
}

#[derive(Clone)]
pub(crate) struct MachineLoginBinding {
    pub handle: MachineLoginHandle,
}
impl MachineLoginBinding {
    pub(crate) fn source(&self) -> MachineTokenSource {
        self.handle.source.clone()
    }
    pub(crate) fn handle(&self) -> MachineLoginHandle {
        self.handle.clone()
    }
}

pub(crate) struct HostMachineLogin {
    owner: RunningMachineLogin,
    edge: EdgeCredential,
    peer: PeerId,
}
impl HostMachineLogin {
    pub(crate) fn binding(&self) -> MachineLoginBinding {
        MachineLoginBinding {
            handle: self.owner.handle.clone(),
        }
    }
    pub(crate) fn route(&self) -> MachineLoginRoute {
        self.owner.handle.route()
    }
    pub(crate) fn session_inputs(&self) -> io::Result<Option<BoardSessionInputs>> {
        let route = self.route();
        let Some(saved) = route.saved() else {
            return Ok(None);
        };
        let EdgeCredential::Ready(access) = &self.edge else {
            return Err(io::Error::other(
                "the configured Fleet Access reference is unavailable",
            ));
        };
        Ok(Some(BoardSessionInputs {
            endpoint: Endpoint::new(
                super::issuer_discovery::session_address(&saved.sign_in)
                    .map_err(|_| io::Error::other("invalid saved Fleet sign-in address"))?
                    .as_str(),
            )
            .map_err(io::Error::other)?,
            access: access.clone(),
            // Stock SessionPump requires a fixed argument. Machine-token
            // transport ignores this public marker and reads the live source.
            credential: PeerCredential::new("host-token-source").map_err(io::Error::other)?,
            peer: self.peer,
            receiver: saved.central.receiver,
            relay: saved.central.relay,
        }))
    }
    pub(crate) async fn shutdown(self) -> io::Result<()> {
        self.owner.shutdown().await
    }
}

pub(crate) async fn start_machine_login(
    secret_root: &Path,
    directory: &Path,
    edge_reference: MachineEdgeAccess,
    inputs: MachineTokenStartupInputs,
) -> io::Result<HostMachineLogin> {
    use std::os::unix::fs::DirBuilderExt;
    match std::fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let repository = inputs.repository;
    let saved = repository
        .load_fleet_login()
        .await
        .map_err(io::Error::other)?;
    let installation: fleet_identity_protocol::InstallationId =
        String::from(collaboration_runtime::load_service_identity(directory)?)
            .try_into()
            .map_err(|_| io::Error::other("invalid saved installation identity"))?;
    let peer = collaboration_runtime::load_installation_peer(directory)?;
    let (clock, keychain, issuer_client): (
        Arc<dyn FleetLoginClock>,
        Option<Arc<dyn KeychainAccess>>,
        reqwest::Client,
    ) = match inputs.dependencies {
        Some(dependencies) => (
            dependencies.clock,
            Some(dependencies.keychain),
            dependencies.issuer_client,
        ),
        None => (
            Arc::new(HostFleetLoginClock),
            None,
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|_| io::Error::other("machine issuer client is unavailable"))?,
        ),
    };
    let root = secret_root.to_owned();
    let classified = tokio::task::spawn_blocking(move || match keychain {
        Some(keychain) => {
            ClassifiedCredentialStore::open_read_only_with_keychain(&root, keychain.as_ref())
        }
        None => ClassifiedCredentialStore::open_read_only_for_process(&root),
    })
    .await
    .map_err(io::Error::other)?;
    let credentials = Arc::new(RefreshCredentialStore::from_classification(
        classified,
        installation.clone(),
    ));
    let edge = match read_edge_credential(credentials.as_ref(), edge_reference) {
        Ok(access) => EdgeCredential::Ready(access),
        Err(()) => EdgeCredential::Unavailable,
    };
    let publication = Arc::new(MachineLoginPublication {
        repository,
        credentials,
        #[cfg(any(test, feature = "runtime-test-support"))]
        after_commit: None,
    });
    let work = MachineLoginWork {
        issuer: Arc::new(HttpMachineIssuerConnection {
            client: issuer_client,
            clock: clock.clone(),
        }),
        clock,
        publication,
    };
    let owner =
        RunningMachineLogin::start(MachineLoginIdentity { installation, peer }, work, saved);
    Ok(HostMachineLogin { owner, edge, peer })
}

fn read_edge_credential(
    credentials: &RefreshCredentialStore,
    reference: MachineEdgeAccess,
) -> Result<AccessServiceToken, ()> {
    let read = |key: SecretKey, kind: FleetSecretKind| {
        let RefreshCredentialStore::Encrypted {
            store,
            installation,
        } = credentials
        else {
            return Err(());
        };
        let identity = FleetSecretKey::parse(&key).map_err(|_| ())?.ok_or(())?;
        if identity.kind() != kind || identity.installation() != installation.as_str() {
            return Err(());
        }
        store.read_secret(&key).map_err(|_| ())
    };
    match reference {
        MachineEdgeAccess::Unconfigured => Ok(AccessServiceToken::NoEdgeCredentials),
        MachineEdgeAccess::Cloudflare {
            client_id,
            secret_key,
        } => {
            let secret = read(secret_key, FleetSecretKind::AccessClientSecret)?;
            AccessServiceToken::new(&client_id, secret.expose_secret()).map_err(|_| ())
        }
        MachineEdgeAccess::SelfHosted { assertion_key } => {
            let secret = read(assertion_key, FleetSecretKind::MachineAssertion)?;
            AccessServiceToken::self_hosted(secret.expose_secret()).map_err(|_| ())
        }
    }
}

#[cfg(test)]
#[path = "startup_tests.rs"]
mod tests;
