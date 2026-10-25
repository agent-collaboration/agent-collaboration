use message_board::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
mod support;

#[allow(clippy::expect_used)] // This helper asserts a test contract, not a recovery path.
fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: T) {
    let encoded = serde_json::to_value(&value).expect("serialize contract");
    assert_eq!(
        serde_json::from_value::<T>(encoded).expect("decode contract"),
        value
    );
}

#[test]
fn active_names_and_thread_subjects_are_closed() {
    roundtrip(WorkspaceState::Archiving);
    roundtrip(BoardKind::Initiative {
        goal: "Deliver board records".into(),
        done_when: "Proof passes".into(),
    });
    roundtrip(BoardKind::Area {});
    roundtrip(ThreadState::Open);
    roundtrip(ThreadSubject::Workstream(WorkstreamId::generate()));
    roundtrip(ThreadSubject::Task(TaskId::generate()));
    roundtrip(ThreadSubject::WorkTrailRow(WorkTrailRowId::generate()));
    roundtrip(ThreadSubject::WorkspaceAdr(AdrId::generate()));
    assert!(
        serde_json::from_value::<ThreadSubject>(
            json!({"kind":"milestone","subject":TaskId::generate()})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<BoardKind>(json!({"kind":"area","goal":"hidden"})).is_err());
    assert!(serde_json::from_value::<RecordKind>(json!("brief")).is_err());
    assert!(
        serde_json::from_value::<UnavailableReason>(
            json!({"kind":"boardUnreachable","hidden":true})
        )
        .is_err()
    );
}

#[test]
fn uuid_ids_and_counter_limits_are_validated_at_decode() {
    let id = PostId::generate();
    assert!(PostId::try_from(String::new()).is_err());
    assert!(serde_json::from_value::<PostId>(json!(id.as_str().to_uppercase())).is_err());
    assert!(EpochNumber::try_from(0).is_err());
    assert!(FeedPosition::try_from(i64::MAX as u64 + 1).is_err());
    assert!(SubjectRevision::try_from(i64::MAX as u64 + 1).is_err());
    roundtrip(RecordPlacement::Project(ProjectFeedEntry {
        cursor: FeedCursor {
            epoch: EpochNumber::try_from(1).expect("initial epoch"),
            position: FeedPosition::ZERO,
        },
        origin: support::actor_origin(),
    }));
    roundtrip(RecordPlacement::Fleet(FleetFeedEntry {
        position: FleetPosition::ZERO,
        origin: support::actor_origin(),
    }));
    roundtrip(RequestResult::LeaseExpired {
        expired_at: "2026-10-12T00:00:00Z".parse().expect("UTC timestamp"),
    });
    roundtrip(RequestResult::Decided(RequestDecision::Rejected {
        reasons: vec![Refusal::Task(TaskRefusal::DependencyMissing(
            TaskId::generate(),
        ))],
    }));
    assert!(serde_json::from_value::<RequestDecision>(json!({"kind":"pending"})).is_err());
}

#[test]
fn role_subject_constructor_and_deserializer_reject_every_wrong_pair() {
    let actor = RecordedActor::declared(Identity::Human {
        human_id: HumanId::try_from("writer".to_owned()).expect("human id"),
    });
    for subject in [
        RoleSubject::Project(ProjectId::generate()),
        RoleSubject::Workstream(WorkstreamId::generate()),
    ] {
        for role in [
            AuthorityRole::Lead,
            AuthorityRole::Implementer,
            AuthorityRole::Verifier,
        ] {
            let valid = matches!(
                (&subject, role),
                (RoleSubject::Project(_), AuthorityRole::Lead)
                    | (
                        RoleSubject::Workstream(_),
                        AuthorityRole::Implementer | AuthorityRole::Verifier
                    )
            );
            let props = RoleAssignmentProps {
                envelope: support::record_envelope(RecordId::generate(), ProjectId::generate()),
                assignment_id: RoleAssignmentId::generate(),
                subject: subject.clone(),
                role,
                holder: actor.clone(),
                granted_by: actor.clone(),
                model: None,
                lineage: None,
                ended: None,
                owner_grant: None,
            };
            assert_eq!(RoleAssignment::new(props.clone()).is_ok(), valid);
            assert_eq!(
                serde_json::from_value::<RoleAssignment>(
                    serde_json::to_value(props).expect("props")
                )
                .is_ok(),
                valid
            );
            if let Ok(assignment) = RoleAssignment::new(RoleAssignmentProps {
                envelope: support::record_envelope(RecordId::generate(), ProjectId::generate()),
                assignment_id: RoleAssignmentId::generate(),
                subject: subject.clone(),
                role,
                holder: actor.clone(),
                granted_by: actor.clone(),
                model: None,
                lineage: None,
                ended: None,
                owner_grant: None,
            }) {
                roundtrip(assignment);
            }
        }
    }
}

#[test]
fn role_schema_names_its_subject_and_authority() {
    let schema = schemars::schema_for!(RoleAssignment);
    let properties = schema.get("properties").unwrap();
    assert!(properties.get("subject").is_some());
    assert!(properties.get("role").is_some());
    assert!(properties.get("assignment").is_none());
}
