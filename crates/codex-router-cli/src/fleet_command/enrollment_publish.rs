//! Generation-specific secret references are activated only by the metadata rename.
use super::{FleetCommandError, enrollment_prepare::PreparedEnrollment};
use codex_router_host::{
    FleetAccessReference, FleetEnrollmentDocument, FleetEnrollmentPublishError,
};
use codex_router_secret_store::{
    SecretStore,
    fleet_secret_key::{FleetSecretKey, FleetSecretKind},
    model::SecretKey,
};
use collaboration_central_core::EnrollmentDecision;

pub(super) fn publish(
    prepared: PreparedEnrollment,
    decision: &EnrollmentDecision,
) -> Result<(), FleetCommandError> {
    let EnrollmentDecision::MachineEnrolled {
        peer,
        expires_at,
        authoritative_receiver,
        relay_peer,
    } = decision
    else {
        return Err(FleetCommandError::InvalidInput(
            "only a successful enrollment can publish local state",
        ));
    };
    if *peer != prepared.request.peer {
        return Err(FleetCommandError::OutcomeUnknown(
            "central returned a different installation peer",
        ));
    }
    let secret_root = codex_router_host::HostCoordinationPaths::for_router_root(&prepared.root)
        .router_secret_root()
        .ok_or(FleetCommandError::InvalidInput(
            "Router secret root is unavailable",
        ))?;
    #[cfg(debug_assertions)]
    if prepared.allow_plaintext_file_secrets {
        crate::secret_store_factory::initialize_plaintext_debug_root(&secret_root)?;
    }
    let store = crate::secret_store_factory::open_cli_secret_store(&secret_root)?;
    let generation = String::from(collaboration_service::new_service_uuid()?);
    let installation = prepared.request.installation.as_str();
    let access_key = FleetSecretKey::new(
        FleetSecretKind::AccessClientSecret,
        installation,
        &generation,
    )?
    .secret_key()?;
    let credential_key = FleetSecretKey::new(
        FleetSecretKind::MachineCredential,
        installation,
        &generation,
    )?
    .secret_key()?;
    if let Err(error) = store.write_staged(&access_key, &prepared.access_secret) {
        cleanup(&store, &access_key, &credential_key)?;
        return Err(error.into());
    }
    if let Err(error) = store.write_staged(&credential_key, &prepared.machine_credential) {
        cleanup(&store, &access_key, &credential_key)?;
        return Err(error.into());
    }
    let document = FleetEnrollmentDocument {
        endpoint: prepared.endpoint,
        access: FleetAccessReference {
            client_id: prepared.request.access_client_id.as_str().to_owned(),
            client_secret_key: access_key.as_str().to_owned(),
        },
        credential_key: credential_key.as_str().to_owned(),
        peer: *peer,
        receiver: *authoritative_receiver,
        relay: *relay_peer,
        expires_at: Some(*expires_at),
    };
    match codex_router_host::persist_fleet_enrollment(&prepared.root, &document, &store) {
        Ok(()) => Ok(()),
        Err(error @ FleetEnrollmentPublishError::BeforeRename(_)) => {
            cleanup(&store, &access_key, &credential_key)?;
            Err(FleetCommandError::Publication(error))
        }
        Err(error @ FleetEnrollmentPublishError::Renamed(_)) => {
            Err(FleetCommandError::Publication(error))
        }
    }
}

fn cleanup(
    store: &dyn SecretStore,
    access: &SecretKey,
    credential: &SecretKey,
) -> Result<(), FleetCommandError> {
    let access_removed = store.delete_staged(access);
    let credential_removed = store.delete_staged(credential);
    if access_removed.is_err() || credential_removed.is_err() {
        return Err(FleetCommandError::CleanupUnconfirmed);
    }
    Ok(())
}
