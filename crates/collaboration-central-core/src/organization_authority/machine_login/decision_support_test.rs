use super::super::policy_fixture::*;
use super::super::*;
use super::*;
use crate::identity_contract::MachineTokenDecision;

#[test]
fn generation_exhaustion_and_invalid_id_sources_fail_without_a_plan() {
    let max = EnrollmentGeneration::try_from(9_007_199_254_740_991).unwrap();
    assert!(max.next().is_err());
    assert!(EnrollmentGeneration::try_from(9_007_199_254_740_992).is_err());
    let facts = facts(approved(40, max.get()), None, max.get());
    assert!(
        decide_machine_token(
            &token(facts.enrollment.as_ref().unwrap()),
            &facts,
            time(NOW_MS),
            &CountingIds::new(),
            &settings()
        )
        .is_err()
    );
    struct InvalidIds;
    impl IdSource for InvalidIds {
        fn new_id(&self) -> durable_steps::Uuid {
            uuid::Uuid::nil()
        }
    }
    assert!(
        decide_device_login(
            &approval(),
            &empty_facts(),
            NOW_MS,
            time(NOW_MS),
            &InvalidIds
        )
        .is_err()
    );
}

#[test]
fn explicit_typed_lifetimes_control_grants_and_od2_production_is_six_hundred_seconds() {
    let facts = facts(approved(40, 0), None, 0);
    for seconds in [60_u32, 900, 3600] {
        let plan = decide_machine_token(
            &token(facts.enrollment.as_ref().unwrap()),
            &facts,
            time(NOW_MS),
            &CountingIds::new(),
            &MachineLoginSettings::new(seconds.try_into().unwrap()),
        )
        .unwrap();
        assert!(
            matches!(plan.answer, MachineTokenDecision::Grant { decided_at, grant_expires_at, .. } if decided_at.0 == NOW_S && grant_expires_at.0 == NOW_S+i64::from(seconds))
        );
    }
    assert_eq!(
        u32::from(MachineLoginSettings::production().grant_lifetime),
        600
    );
}
