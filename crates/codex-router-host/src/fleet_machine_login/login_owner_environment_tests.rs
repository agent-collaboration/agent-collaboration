//! The Host forwards the assertion and adopts the repository's confirmed environment.
use super::*;
use codex_router_fleet_state::fleet_login::FleetLoginCandidate;
use fleet_identity_protocol::MachineEnvironment::{CloudVm, Physical};

// Oracle: PD bb4c7145 and the approved repository boundary. Wrong behavior:
// the Host substitutes physical for omission, or drops an explicit assertion.
// The memory repository stands in for the held transaction; real persistence
// and lifecycle reassembly are outside this test. No pre-fix red observed.
#[tokio::test]
async fn fleet_machine_login_host_adopts_the_repository_environment_without_defaulting_early() {
    for (initial, requested, expected) in [
        (None, None, Physical),
        (None, Some(CloudVm), CloudVm),
        (Some(CloudVm), None, CloudVm),
        (Some(CloudVm), Some(Physical), Physical),
    ] {
        // Arrange
        let fixture = Fixture::new();
        if let Some(environment) = initial {
            let previous = saved("019f0000-0000-7000-8000-000000000009");
            fixture
                .repository
                .replace_fleet_login(&FleetLoginCandidate {
                    sign_in: previous.sign_in,
                    organization_id: previous.organization_id,
                    enrollment_id: previous.enrollment_id,
                    requested_machine_environment: Some(environment),
                    central: previous.central,
                })
                .await
                .unwrap();
        }
        let running = fixture.start().await;
        let mut events = running.handle.observe(
            Some(SIGN_IN.to_owned().try_into().unwrap()),
            ORGANIZATION.to_owned().try_into().unwrap(),
            "Fixture machine".to_owned().try_into().unwrap(),
            requested,
        );
        assert!(matches!(
            events.recv().await,
            Some(FleetLoginEvent::Code { .. })
        ));

        // Act
        fixture.issue();
        assert!(matches!(
            fixture.next(&running, &mut events).await,
            Some(FleetLoginEvent::LoggedIn { .. })
        ));

        // Assert
        let confirmed = fixture
            .repository
            .load_fleet_login()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(confirmed.machine_environment, expected);
        assert_eq!(confirmed.enrollment_id.as_str(), ENROLLMENT);
        assert_eq!(confirmed.sign_in.as_str(), SIGN_IN);
        assert_eq!(running.handle.route().saved().unwrap().as_ref(), &confirmed);
        running.shutdown().await.unwrap();
    }
}
