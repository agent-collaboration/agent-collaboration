//! Local Claim proof stand-in: existing assertion auth shares the admin origin.
//! Real Cloudflare Access remains a staging obligation.
use super::{cutover_host_fixture::CutoverHostFixture, offline_cli_fixture::TestResult};
use board_central_test_support::TestCentral;
use codex_router_core::redaction::SecretString;
use codex_router_host::{
    FleetAccess, SelfHostedAccessReference, persist_fleet_enrollment,
    read_fleet_enrollment_document,
};
use codex_router_secret_store::{
    SecretStore,
    file_backend::FileSecretStore,
    fleet_secret_key::{FleetSecretKey, FleetSecretKind},
};

pub(super) fn install(central: &TestCentral, host: &CutoverHostFixture) -> TestResult {
    let inputs = central.session_inputs()?;
    let credential = inputs
        .credential
        .expose()
        .strip_prefix("Bearer ")
        .ok_or("fixture credential is not a Bearer credential")?;
    let assertion = collaboration_central_conformance::machine_assertion(credential)?;
    let path = central.write_host_session(&host.root)?;
    let mut document = read_fleet_enrollment_document(&path, &host.root)?;
    let (_, credential_key) = document.secret_keys()?;
    let identity =
        FleetSecretKey::parse(&credential_key)?.ok_or("fixture credential identity missing")?;
    let service: String = host.service.clone().into();
    if identity.installation() != service.as_str() {
        return Err("fixture enrollment belongs to another service".into());
    }
    let assertion_key = FleetSecretKey::new(
        FleetSecretKind::MachineAssertion,
        &identity.installation(),
        &identity.generation(),
    )?
    .secret_key()?;
    let store = FileSecretStore::initialize_debug_plaintext(&host.root.join("secrets"))?;
    store.write_secret(&assertion_key, &SecretString::new(assertion))?;
    document.endpoint = format!("{}/v1/session", central.base_url());
    document.access = FleetAccess::SelfHosted(SelfHostedAccessReference::SelfHosted {
        assertion_key: assertion_key.as_str().to_owned(),
    });
    persist_fleet_enrollment(&host.root, &document, &store)?;
    std::fs::remove_file(path)?;
    Ok(())
}
