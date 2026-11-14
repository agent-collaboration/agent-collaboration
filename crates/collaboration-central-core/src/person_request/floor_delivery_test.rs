//! Pure floor policy cases; real storage/commit assertions belong to host integration.
use super::{FloorDelivery, FloorMutation, decide_floor_delivery};
use crate::{
    identity_contract::AuthorizationRevision,
    person_request::{OrganizationFloor, OrganizationFloorState, PersonFloor, PersonFloorState},
};
use serde_json::json;

const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000f201";
const PERSON: &str = "019f0000-0000-7000-8000-00000000f202";
const SUBJECT: &str = "019f0000-0000-7000-8000-00000000f203";
const POLICY: &str = "2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22";

fn organization(revision: i64, state: &str) -> AuthorizationRevision {
    serde_json::from_value(json!({"kind":"organization","organizationId":ORGANIZATION,"revision":revision,"state":state,"permissionPolicyDigest":POLICY})).unwrap()
}
fn person(revision: i64, state: &str, subject: &str) -> AuthorizationRevision {
    serde_json::from_value(json!({"kind":"person","organizationId":ORGANIZATION,"personId":PERSON,"subjectId":subject,"revision":revision,"state":state})).unwrap()
}
fn floor(revision: i64, state: OrganizationFloorState) -> OrganizationFloor {
    OrganizationFloor {
        revision: revision.try_into().unwrap(),
        state,
        policy_digest: POLICY.into(),
        reject_through_ms: 0,
    }
}
fn acknowledged(revision: i64) -> FloorDelivery {
    FloorDelivery::Acknowledged(revision.try_into().unwrap())
}

#[test]
fn organization_floor_policy_raises_never_lowers_and_never_resumes_deleted() {
    // Historical response oracle: first/repeat/conflict/older/stopped/deleted/resume.
    let cases = [
        (None, organization(2, "active"), acknowledged(2), true),
        (
            Some(floor(2, OrganizationFloorState::Active)),
            organization(2, "active"),
            acknowledged(2),
            false,
        ),
        (
            Some(floor(2, OrganizationFloorState::Active)),
            organization(2, "stopped"),
            FloorDelivery::Conflict,
            false,
        ),
        (
            Some(floor(2, OrganizationFloorState::Active)),
            organization(1, "active"),
            acknowledged(2),
            false,
        ),
        (
            Some(floor(2, OrganizationFloorState::Active)),
            organization(3, "stopped"),
            acknowledged(3),
            true,
        ),
        (
            Some(floor(3, OrganizationFloorState::Stopped)),
            organization(5, "deleted"),
            acknowledged(5),
            true,
        ),
        (
            Some(floor(5, OrganizationFloorState::Deleted)),
            organization(6, "active"),
            acknowledged(5),
            false,
        ),
    ];
    for (current, incoming, expected, writes) in cases {
        let (delivery, mutation) =
            decide_floor_delivery(&incoming, current.as_ref(), None).unwrap();
        assert_eq!(delivery, expected);
        assert_eq!(
            mutation,
            if writes {
                FloorMutation::Store(incoming)
            } else {
                FloorMutation::Unchanged
            }
        );
    }
    let mut another_policy = floor(2, OrganizationFloorState::Active);
    another_policy.policy_digest = "ab".repeat(32);
    assert_eq!(
        decide_floor_delivery(&organization(2, "active"), Some(&another_policy), None).unwrap(),
        (FloorDelivery::Conflict, FloorMutation::Unchanged)
    );
}

#[test]
fn person_floor_policy_needs_org_and_keeps_one_subject_per_person() {
    let organization = floor(1, OrganizationFloorState::Active);
    let incoming = person(1, "active", SUBJECT);
    assert_eq!(
        decide_floor_delivery(&incoming, None, None).unwrap(),
        (
            FloorDelivery::OrganizationFloorMissing,
            FloorMutation::Unchanged
        )
    );
    assert_eq!(
        decide_floor_delivery(&incoming, Some(&organization), None).unwrap(),
        (acknowledged(1), FloorMutation::Store(incoming))
    );
    let active = PersonFloor {
        subject_id: SUBJECT.into(),
        revision: 1.try_into().unwrap(),
        state: PersonFloorState::Active,
    };
    let inactive = PersonFloor {
        subject_id: SUBJECT.into(),
        revision: 4.try_into().unwrap(),
        state: PersonFloorState::Inactive,
    };
    for (current, incoming, expected, writes) in [
        (
            &active,
            person(1, "active", SUBJECT),
            acknowledged(1),
            false,
        ),
        (
            &active,
            person(4, "inactive", SUBJECT),
            acknowledged(4),
            true,
        ),
        (
            &inactive,
            person(2, "active", SUBJECT),
            acknowledged(4),
            false,
        ),
        (
            &inactive,
            person(4, "active", SUBJECT),
            FloorDelivery::Conflict,
            false,
        ),
        (
            &inactive,
            person(9, "active", "019f0000-0000-7000-8000-00000000f2ff"),
            FloorDelivery::Conflict,
            false,
        ),
    ] {
        let (delivery, mutation) =
            decide_floor_delivery(&incoming, Some(&organization), Some(current)).unwrap();
        assert_eq!(delivery, expected);
        assert_eq!(
            mutation,
            if writes {
                FloorMutation::Store(incoming)
            } else {
                FloorMutation::Unchanged
            }
        );
    }
}

#[test]
fn floor_delivery_refuses_out_of_range_revisions_and_unknown_states_at_the_wire() {
    for body in [
        json!({"kind":"organization","organizationId":ORGANIZATION,"revision":0,"state":"active","permissionPolicyDigest":POLICY}),
        json!({"kind":"organization","organizationId":ORGANIZATION,"revision":9_007_199_254_740_992_i64,"state":"active","permissionPolicyDigest":POLICY}),
        json!({"kind":"organization","organizationId":ORGANIZATION,"revision":1,"state":"paused","permissionPolicyDigest":POLICY}),
        json!({"kind":"person","organizationId":ORGANIZATION,"personId":PERSON,"subjectId":SUBJECT,"revision":1,"state":"removed"}),
        json!({"kind":"person","organizationId":ORGANIZATION,"personId":PERSON,"subjectId":SUBJECT,"revision":1,"state":"active","acknowledgedRevision":2}),
    ] {
        assert!(
            serde_json::from_value::<AuthorizationRevision>(body.clone()).is_err(),
            "{body}"
        );
    }
}
