use message_board::{
    ProjectId, ProjectRegistrationRejection, ProjectRejection, Refusal, ScopeRefusal,
};
use serde_json::json;

#[test]
fn rejection_preserves_the_project_and_opaque_central_reason() {
    let rejection = ScopeRefusal::ProjectRejected {
        project: ProjectId::generate(),
        reason: ProjectRejection::Registration {
            reason: ProjectRegistrationRejection {
                code: "consumer.futureReason".to_owned(),
                detail: " central detail is opaque {\"kind\":\"archived\"} ".to_owned(),
            },
        },
    };
    let value = serde_json::to_value(&rejection).unwrap();
    assert_eq!(value["kind"], json!("projectRejected"));
    assert!(value.get("creatingRequest").is_none());
    assert_eq!(
        serde_json::from_value::<ScopeRefusal>(value).unwrap(),
        rejection
    );
}

#[test]
fn rejection_refuses_missing_fields_and_removed_creation_bookkeeping() {
    let valid = json!({"kind":"projectRejected","project":ProjectId::generate(),"reason":{"kind":"registration","reason":{"code":"central.reason","detail":"opaque"}}});
    for pointer in [
        "/project",
        "/reason/kind",
        "/reason/reason/code",
        "/reason/reason/detail",
    ] {
        let mut invalid = valid.clone();
        let (parent, field) = pointer.rsplit_once('/').unwrap();
        invalid
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<ScopeRefusal>(invalid).is_err());
    }
    let mut legacy = valid;
    legacy["creatingRequest"] = json!(message_board::RequestId::generate());
    assert!(serde_json::from_value::<ScopeRefusal>(legacy).is_err());
}

#[test]
fn directory_rejection_keeps_structured_evidence_and_refuses_the_old_opaque_shape() {
    let refusal = Refusal::NameConflict {
        name: message_board::ResourceName::try_from("occupied".to_owned()).unwrap(),
        existing: ProjectId::generate().into(),
    };
    let rejection = ScopeRefusal::ProjectRejected {
        project: ProjectId::generate(),
        reason: ProjectRejection::Directory { refusal },
    };
    let value = serde_json::to_value(&rejection).unwrap();
    assert_eq!(value["reason"]["kind"], json!("directory"));
    assert_eq!(
        serde_json::from_value::<ScopeRefusal>(value).unwrap(),
        rejection
    );
    assert!(
        serde_json::from_value::<ScopeRefusal>(json!({
            "kind":"projectRejected","project":ProjectId::generate(),
            "reason":{"code":"central.reason","detail":"opaque"}
        }))
        .is_err()
    );
}
