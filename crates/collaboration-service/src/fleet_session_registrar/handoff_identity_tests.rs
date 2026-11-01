//! Declared input identity is separate from the persisted F1 registration facts.
use super::*;

#[tokio::test]
async fn fleet_session_registrar_refuses_changed_effort_and_access_under_a_prepared_operation() {
    let fixture = Fixture::new();
    let intent = fixture.intent();
    fixture
        .registrar
        .prepare(&intent, &fixture.identity(&intent))
        .await
        .unwrap();
    let before = fixture
        .repository
        .records
        .lock()
        .unwrap()
        .first()
        .unwrap()
        .clone();
    for change_effort in [true, false] {
        let mut changed = fixture.declared_input(&intent);
        let collaboration_contracts::HarnessDeclaredSettings::Native { effort, access, .. } =
            &mut changed.settings
        else {
            panic!("native fixture settings");
        };
        if change_effort {
            *effort = Some("high".to_owned().try_into().unwrap());
        } else {
            *access = collaboration_protocol::RouterAccess::WriteRestricted;
        }
        let identity = changed.identity().unwrap();

        let answer = fixture.registrar.prepare(&intent, &identity).await;

        assert!(matches!(answer, Err(RegistrarFailure::OperationConflict)));
        assert_eq!(
            fixture.repository.records.lock().unwrap().first(),
            Some(&before)
        );
        assert!(fixture.authority.registrations.lock().unwrap().is_empty());
    }
}
