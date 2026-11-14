use super::super::policy_fixture::*;
use super::*;
use crate::identity_contract::MachineTokenRefusal;
use crate::organization_authority::OrganizationChangeOrigin as Origin;

#[test]
fn owner_revocation_fences_pending_approvals_and_aba_without_a_public_route() {
    let active = enrolled(40, 0, NOW_S);
    let facts = facts(active.clone(), Some(active), 1);
    let operation = id(90).try_into().unwrap();
    let plan = revoke_machine_enrollment(
        &facts,
        &person(),
        &operation,
        time(NOW_MS),
        &CountingIds::new(),
    )
    .unwrap();
    assert_eq!(
        plan.answer,
        Some(EndedMachineClient {
            enrollment_id: id(40).try_into().unwrap(),
            client_id: id(40).try_into().unwrap()
        })
    );
    let next = project(&facts, &plan);
    assert_eq!(next.machine.as_ref().unwrap().generation.get(), 2);
    assert_eq!(next.machine.as_ref().unwrap().state, MachineState::Revoked);
    assert!(next.machine.as_ref().unwrap().allowed_workspaces.is_empty());
    assert_eq!(
        next.enrollment.as_ref().unwrap().state,
        StoredEnrollmentState::Revoked {
            client_id: id(40).try_into().unwrap(),
            reason: EnrollmentRevocationReason::Owner {}
        }
    );
    for record in records(&plan) {
        assert_eq!(
            record.origin,
            Origin::Person {
                person_id: person().as_str().to_owned(),
                permission_checked: "machine:revoke".to_owned()
            }
        );
        assert!(record.request_id.as_str().starts_with(operation.as_str()));
    }
    assert_refused(
        &decide_token(&next, NOW_MS + 1),
        MachineTokenRefusal::EnrollmentNotActive {},
    );
    let stale = EnrollmentFacts {
        enrollment: Some(approved(41, 1)),
        ..next.clone()
    };
    assert_refused(
        &decide_token(&stale, NOW_MS + 1),
        MachineTokenRefusal::Superseded {},
    );
    let fresh = EnrollmentFacts {
        enrollment: Some(approved(42, 2)),
        ..next
    };
    let activated = project(&fresh, &decide_token(&fresh, NOW_MS + 1));
    let aba = EnrollmentFacts {
        enrollment: Some(approved(41, 1)),
        ..activated
    };
    assert_refused(
        &decide_token(&aba, NOW_MS + 2),
        MachineTokenRefusal::Superseded {},
    );
    assert_eq!(
        revoke_machine_enrollment(
            &empty_facts(),
            &person(),
            &operation,
            time(NOW_MS),
            &CountingIds::new()
        )
        .unwrap(),
        Decided::unchanged(None)
    );
}
