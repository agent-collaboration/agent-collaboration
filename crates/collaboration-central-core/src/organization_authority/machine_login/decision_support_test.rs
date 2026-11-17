use super::super::{
    EnrollmentGeneration, decide_device_login, decide_machine_token,
    policy_fixture::{
        CountingIds, NOW_MS, NOW_S, approval, approved, empty_facts, facts, time, token,
    },
};
use crate::identity_contract::MachineTokenDecision;
use runtime_primitives::IdSource;

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
        )
        .is_err()
    );
    struct InvalidIds;
    impl IdSource for InvalidIds {
        fn new_id(&self) -> uuid::Uuid {
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
fn fixed_grant_deadline_uses_the_applying_whole_second() {
    let facts = facts(approved(40, 0), None, 0);
    for subsecond_ms in [0, 1, 999] {
        let plan = decide_machine_token(
            &token(facts.enrollment.as_ref().unwrap()),
            &facts,
            time(NOW_S * 1000 + subsecond_ms),
            &CountingIds::new(),
        )
        .unwrap();
        assert!(
            matches!(plan.answer, MachineTokenDecision::Grant { decided_at, grant_expires_at, .. } if decided_at.0 == NOW_S && grant_expires_at.0 == NOW_S+600)
        );
    }
}
