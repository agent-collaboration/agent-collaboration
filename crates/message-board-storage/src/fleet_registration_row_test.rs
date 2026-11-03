use super::*;
use crate::fleet_session_registration::tests::{preparation, registration};

#[test]
fn typed_registration_rows_preserve_each_started_by_variant_and_setup_cause() {
    let preparation = preparation();
    let session = registration(&preparation).session;
    let started_by = [
        StartedBy::RouterOperator {},
        StartedBy::Session {
            session: session.clone(),
        },
        StartedBy::RemoteSession {
            session: session.clone(),
            request_id: "01900000-0000-7000-8000-000000000001"
                .to_owned()
                .try_into()
                .unwrap(),
        },
    ];
    let causes = [
        SessionSetupAbandonmentCause::ModelMismatch {},
        SessionSetupAbandonmentCause::EffectiveWorkingDirectoryMismatch {},
        SessionSetupAbandonmentCause::WorkspaceRootsMismatch {},
        SessionSetupAbandonmentCause::SetupInterrupted {},
        SessionSetupAbandonmentCause::InitialSettingRefused {
            setting: ProviderSettingName::Mode,
        },
        SessionSetupAbandonmentCause::InitialSettingRefused {
            setting: ProviderSettingName::Model,
        },
        SessionSetupAbandonmentCause::InitialSettingRefused {
            setting: ProviderSettingName::Effort,
        },
    ];
    for started_by in started_by {
        for cause in causes {
            let mut input = preparation.clone();
            input.launch.started_by = started_by.clone();
            let original = FleetSessionRegistrationRecord::new(
                input,
                FleetSessionBindingLifecycle::Abandoned {
                    creation: AbandonedSessionCreation::KnownCreated {
                        session: session.clone(),
                        cause,
                    },
                },
            )
            .unwrap();
            let row = StoredRegistrationRow::encode(&original, 1000, 2000).unwrap();
            assert_eq!(row.decode().unwrap(), original);
        }
    }
}

#[test]
fn typed_registration_rows_reject_unknown_tags_unused_payloads_and_backwards_timestamps() {
    let record = FleetSessionRegistrationRecord::new(
        preparation(),
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::NotDispatched {},
        },
    )
    .unwrap();
    let mut unknown = StoredRegistrationRow::encode(&record, 1000, 1000).unwrap();
    unknown.lifecycle_state = "future".to_owned();
    assert!(matches!(
        unknown.decode(),
        Err(FleetSessionStorageFailure::InvalidRecord)
    ));
    let mut unused = StoredRegistrationRow::encode(&record, 1000, 1000).unwrap();
    unused.remote_start_id = Some("01900000-0000-7000-8000-000000000001".to_owned());
    assert!(matches!(
        unused.decode(),
        Err(FleetSessionStorageFailure::InvalidRecord)
    ));
    let mut backwards = StoredRegistrationRow::encode(&record, 1000, 1000).unwrap();
    backwards.updated_at = 999;
    assert!(matches!(
        backwards.decode(),
        Err(FleetSessionStorageFailure::InvalidRecord)
    ));
}
