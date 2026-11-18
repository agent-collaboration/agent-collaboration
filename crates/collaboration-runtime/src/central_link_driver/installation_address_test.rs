use super::*;

const INSTALLATION: &str = "01890f2e-7b4c-7cc0-98c4-000000000002";
const REMOTE_INSTALLATION: &str = "01890f2e-7b4c-7cc0-98c4-000000000003";
const SERVICE: &str = "80000000-0000-4000-8000-000000000014";

fn wire_address(installation: &str) -> SessionAddress {
    SessionAddress {
        machine_id: installation.parse().unwrap(),
        endpoint: "codex-local".to_owned().try_into().unwrap(),
        session_id: "session-42".to_owned().try_into().unwrap(),
    }
}

#[test]
fn the_local_installation_round_trips_through_its_distinct_service_id() {
    let local = InstallationAddress::new(
        INSTALLATION.parse().unwrap(),
        SERVICE.to_owned().try_into().unwrap(),
    );
    let wire = wire_address(INSTALLATION);
    let session = local.local_session_ref(&wire).unwrap().unwrap();
    assert_eq!(String::from(session.endpoint.service_id.clone()), SERVICE);
    assert_eq!(local.local_session_address(&session).unwrap(), Some(wire));
}

#[test]
fn a_remote_installation_never_becomes_a_local_service_id() {
    let local = InstallationAddress::new(
        INSTALLATION.parse().unwrap(),
        SERVICE.to_owned().try_into().unwrap(),
    );
    assert_eq!(
        local
            .local_session_ref(&wire_address(REMOTE_INSTALLATION))
            .unwrap(),
        None
    );

    let mut session = local
        .local_session_ref(&wire_address(INSTALLATION))
        .unwrap()
        .unwrap();
    session.endpoint.service_id = INSTALLATION.to_owned().try_into().unwrap();
    assert_eq!(local.local_session_address(&session).unwrap(), None);
}
