use super::{
    AdmissionRefusal, FloorRevision, OrganizationFloor, OrganizationFloorState, PersonFloor,
    PersonFloorState, decide_admission,
};
use crate::person_request::{
    AdminOperation, CreateWorkspaceRequest, VerifiedPersonRequest, WorkspaceLookupRequest,
};
use uuid::Uuid;
const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000c201";
const PERSON: &str = "019f0000-0000-7000-8000-00000000c202";
const SUBJECT: &str = "019f0000-0000-7000-8000-00000000c203";
const SESSION: &str = "019f0000-0000-7000-8000-00000000c204";
const POLICY: &str = "2d31b5d72716e64e255d8858c231f593155289fb03ed354c4b762890667a3c22";
const NOW: i64 = 1_900_000_000_000;
/// A request time is a UUIDv7's millisecond prefix.
fn request_at(millis: i64) -> Uuid {
    Uuid::parse_str(&format!(
        "{:08x}-{:04x}-7000-8000-00000000c299",
        millis >> 16,
        millis & 0xffff
    ))
    .unwrap()
}

fn create() -> VerifiedPersonRequest {
    VerifiedPersonRequest {
        person_id: PERSON.into(),
        session_id: SESSION.into(),
        organization_id: ORGANIZATION.to_owned().try_into().unwrap(),
        authorization_subject_id: SUBJECT.into(),
        person_revision: 4,
        organization_revision: 3,
        permission_policy_digest: POLICY.into(),
        permissions: vec!["workspace:create".into(), "organization:view".into()],
        second_factor_at: Some(NOW / 1000 - 60),
        request_id: request_at(NOW - 1_000),
        request_time_ms: NOW - 1_000,
        proof_time_ms: NOW - 1_000,
        proof_key_thumbprint: "thumbprint".into(),
        intent_digest: "intent".into(),
        effective_expiry_ms: NOW + 60_000,
        operation: AdminOperation::CreateWorkspace(CreateWorkspaceRequest {
            organization_id: ORGANIZATION.to_owned().try_into().unwrap(),
            operation_id: "019f0000-0000-7000-8000-00000000c205"
                .to_owned()
                .try_into()
                .unwrap(),
            name: String::from("research").try_into().unwrap(),
            description: String::new(),
        }),
    }
}

fn organization_floor() -> OrganizationFloor {
    OrganizationFloor {
        revision: 3.try_into().unwrap(),
        state: OrganizationFloorState::Active,
        policy_digest: POLICY.into(),
        reject_through_ms: NOW - 10_000,
    }
}
fn person_floor() -> PersonFloor {
    PersonFloor {
        subject_id: SUBJECT.into(),
        revision: 4.try_into().unwrap(),
        state: PersonFloorState::Active,
    }
}
fn policy(
    request: &VerifiedPersonRequest,
    now: i64,
) -> Result<Result<(), AdmissionRefusal>, central_storage::StoreFailure> {
    decide_admission(
        request,
        now,
        Some(&organization_floor()),
        Some(&person_floor()),
        false,
    )
    .map(|decision| decision.map(|_| ()))
}

#[test]
fn missing_floors_and_used_proofs_never_admit() {
    let request = create();
    for (organization, person, expected) in [
        (
            None,
            Some(person_floor()),
            AdmissionRefusal::AuthorizationSyncRequired,
        ),
        (
            Some(organization_floor()),
            None,
            AdmissionRefusal::AuthorizationSyncRequired,
        ),
    ] {
        assert_eq!(
            decide_admission(&request, NOW, organization.as_ref(), person.as_ref(), false)
                .unwrap()
                .unwrap_err(),
            expected
        );
    }
    assert!(policy(&request, NOW).unwrap().is_ok());
    assert_eq!(
        decide_admission(
            &request,
            NOW,
            Some(&organization_floor()),
            Some(&person_floor()),
            true
        )
        .unwrap()
        .unwrap_err(),
        AdmissionRefusal::RequestReused
    );
}

#[test]
fn token_floor_policy_and_subject_bindings_keep_the_historical_refusals() {
    let mut older_org = create();
    older_org.organization_revision = 2;
    let mut newer_org = create();
    newer_org.organization_revision = 4;
    let mut older_person = create();
    older_person.person_revision = 3;
    let mut newer_person = create();
    newer_person.person_revision = 5;
    let mut other_policy = create();
    other_policy.permission_policy_digest = "ab".repeat(32);
    let mut other_subject = create();
    other_subject.authorization_subject_id = "019f0000-0000-7000-8000-00000000c2ff".into();
    for (request, expected) in [
        (older_org, AdmissionRefusal::RefreshRequired),
        (newer_org, AdmissionRefusal::AuthorizationSyncRequired),
        (older_person, AdmissionRefusal::RefreshRequired),
        (newer_person, AdmissionRefusal::AuthorizationSyncRequired),
        (other_policy, AdmissionRefusal::RefreshRequired),
        (other_subject, AdmissionRefusal::NotPermitted),
    ] {
        assert_eq!(policy(&request, NOW).unwrap(), Err(expected));
    }
    for state in [
        OrganizationFloorState::Stopped,
        OrganizationFloorState::Deleted,
    ] {
        let mut organization = organization_floor();
        organization.state = state;
        assert_eq!(
            decide_admission(
                &create(),
                NOW,
                Some(&organization),
                Some(&person_floor()),
                false
            )
            .unwrap()
            .unwrap_err(),
            AdmissionRefusal::NotPermitted
        );
    }
    let mut person = person_floor();
    person.state = PersonFloorState::Inactive;
    assert_eq!(
        decide_admission(
            &create(),
            NOW,
            Some(&organization_floor()),
            Some(&person),
            false
        )
        .unwrap()
        .unwrap_err(),
        AdmissionRefusal::NotPermitted
    );
}

#[test]
fn permission_and_second_factor_are_checked_before_replay() {
    let mut member = create();
    member.permissions = vec!["organization:view".into()];
    let mut stale = create();
    stale.second_factor_at = Some(NOW / 1000 - 901);
    let mut absent = create();
    absent.second_factor_at = None;
    let mut read = create();
    read.second_factor_at = None;
    read.operation = AdminOperation::WorkspaceLookup(WorkspaceLookupRequest {
        organization_id: ORGANIZATION.to_owned().try_into().unwrap(),
        workspace_id: "019f0000-0000-7000-8000-00000000c206"
            .to_owned()
            .try_into()
            .unwrap(),
    });
    for (request, expected) in [
        (member.clone(), Err(AdmissionRefusal::NotPermitted)),
        (stale, Err(AdmissionRefusal::SecondFactorRequired)),
        (absent, Err(AdmissionRefusal::SecondFactorRequired)),
        (read, Ok(())),
    ] {
        assert_eq!(policy(&request, NOW).unwrap(), expected);
    }
    assert_eq!(
        decide_admission(
            &member,
            NOW,
            Some(&organization_floor()),
            Some(&person_floor()),
            true
        )
        .unwrap()
        .unwrap_err(),
        AdmissionRefusal::NotPermitted
    );
    let mut boundary = create();
    boundary.second_factor_at = Some(NOW / 1000 - 900);
    assert_eq!(policy(&boundary, NOW).unwrap(), Ok(()));
    boundary.second_factor_at = Some(NOW / 1000 + 1);
    assert_eq!(
        policy(&boundary, NOW).unwrap(),
        Err(AdmissionRefusal::SecondFactorRequired)
    );
}

#[test]
fn applying_sample_controls_expiry_watermark_and_both_request_windows() {
    let request = create();
    let mut at_watermark = create();
    at_watermark.request_id = request_at(NOW - 10_000);
    at_watermark.request_time_ms = NOW - 10_000;
    for (request, now, expected) in [
        (request.clone(), NOW + 60_000, AdmissionRefusal::Expired),
        (request.clone(), NOW + 64_000, AdmissionRefusal::Expired),
        (request.clone(), NOW - 6_001, AdmissionRefusal::RequestTime),
        (at_watermark, NOW, AdmissionRefusal::RequestTime),
    ] {
        assert_eq!(policy(&request, now).unwrap(), Err(expected));
    }
    let mut late = create();
    late.effective_expiry_ms = NOW + 120_000;
    assert_eq!(
        policy(&late, NOW + 64_000).unwrap(),
        Err(AdmissionRefusal::RequestTime)
    );
    let mut stale_proof = create();
    stale_proof.proof_time_ms = NOW - 65_000;
    assert_eq!(
        policy(&stale_proof, NOW).unwrap(),
        Err(AdmissionRefusal::RequestTime)
    );
    let mut future_proof = create();
    future_proof.proof_time_ms = NOW + 5_001;
    assert_eq!(
        policy(&future_proof, NOW).unwrap(),
        Err(AdmissionRefusal::RequestTime)
    );
    // Time remains first, even if no pushed floor has arrived.
    assert_eq!(
        decide_admission(&request, NOW + 60_000, None, None, false)
            .unwrap()
            .unwrap_err(),
        AdmissionRefusal::Expired
    );
}

#[test]
fn corrupt_floor_domain_values_are_rejected() {
    for revision in [0, -1, 9_007_199_254_740_992] {
        assert!(FloorRevision::try_from(revision).is_err());
    }
    assert_eq!(
        FloorRevision::try_from(9_007_199_254_740_991)
            .unwrap()
            .get(),
        9_007_199_254_740_991
    );
    assert!(OrganizationFloorState::parse("paused").is_err());
    assert!(PersonFloorState::parse("gone").is_err());
}

#[test]
fn persisted_floor_rows_fail_closed_on_wrong_storage_types_and_pair_binding() {
    use crate::authorization_floor_rows::{decode_organization_floor, decode_person_floor};
    use central_storage::{ReadOutcome, SafeInteger, SqlValue};
    let number = |value| SqlValue::Integer(SafeInteger::new(value).unwrap());
    let text = |value: &str| SqlValue::Text(value.to_owned());
    let organization = ReadOutcome {
        columns: [
            "id",
            "minimum_revision",
            "state",
            "permission_policy_digest",
            "request_reject_through_ms",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        rows: vec![vec![
            text(ORGANIZATION),
            number(3),
            text("active"),
            text(POLICY),
            number(NOW - 10_000),
        ]],
    };
    assert_eq!(
        decode_organization_floor(&organization, ORGANIZATION).unwrap(),
        Some(organization_floor())
    );
    for (index, value) in [
        (0, text(PERSON)),
        (1, text("3")),
        (1, number(0)),
        (2, text("paused")),
        (3, text("")),
        (4, number(-1)),
        (4, text("0")),
    ] {
        let mut broken = organization.clone();
        *broken.rows.first_mut().unwrap().get_mut(index).unwrap() = value;
        assert!(decode_organization_floor(&broken, ORGANIZATION).is_err());
    }
    let person = ReadOutcome {
        columns: [
            "id",
            "organization_id",
            "person_id",
            "minimum_revision",
            "state",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        rows: vec![vec![
            text(SUBJECT),
            text(ORGANIZATION),
            text(PERSON),
            number(4),
            text("active"),
        ]],
    };
    assert_eq!(
        decode_person_floor(&person, ORGANIZATION, PERSON).unwrap(),
        Some(person_floor())
    );
    for (index, value) in [
        (0, text("019f0000-0000-4000-8000-00000000c203")),
        (1, text(PERSON)),
        (2, text(ORGANIZATION)),
        (3, text("4")),
        (3, number(0)),
        (4, text("gone")),
    ] {
        let mut broken = person.clone();
        *broken.rows.first_mut().unwrap().get_mut(index).unwrap() = value;
        assert!(decode_person_floor(&broken, ORGANIZATION, PERSON).is_err());
    }
    let mut duplicate = person.clone();
    duplicate.rows.extend(person.rows);
    assert!(decode_person_floor(&duplicate, ORGANIZATION, PERSON).is_err());
}
