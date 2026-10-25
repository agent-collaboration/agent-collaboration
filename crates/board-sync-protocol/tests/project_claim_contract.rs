//! Literal claim names preserve native replication identities and reject loose inputs.
#![allow(clippy::panic_in_result_fn)]
use board_sync_protocol::{ProjectClaimRefusal, ProjectClaimReply, ProjectClaimRequest};
use fleet_sync_protocol::{
    CommitSequence, Epoch, PeerId, ReplicationPosition, SchemaIdentity, SyncTime, TransferId,
};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const ID: &str = "019f0000-0000-7000-8000-000000009700";

#[test]
fn claim_request_authorizes_the_installation_without_a_declared_actor() -> TestResult {
    let value = json!({"requestId":ID,"protocolVersion":1,"projectId":ID});
    let request: ProjectClaimRequest = serde_json::from_value(value.clone())?;
    assert_eq!(serde_json::to_value(request)?, value);
    let mut extra = value.clone();
    extra["actor"] = json!({"kind":"human","humanId":ID});
    assert!(serde_json::from_value::<ProjectClaimRequest>(extra).is_err());
    for field in ["requestId", "protocolVersion", "projectId"] {
        let mut missing = value.clone();
        missing
            .as_object_mut()
            .ok_or("request object")?
            .remove(field);
        assert!(serde_json::from_value::<ProjectClaimRequest>(missing).is_err());
    }
    Ok(())
}

#[test]
fn claim_grant_and_all_six_refusals_keep_exact_native_payloads() -> TestResult {
    let claimed = ProjectClaimReply::Claimed {
        epoch: Epoch::new(2)?,
        expires_at: SyncTime::from_unix_millis(1900014400000),
        start: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(7)),
    };
    let expected = json!({"kind":"claimed","value":{"epoch":2,"expiresAt":1900014400000_i64,"start":{"epoch":1,"sequence":7}}});
    assert_eq!(serde_json::to_value(&claimed)?, expected);
    assert_eq!(
        serde_json::from_value::<ProjectClaimReply>(expected)?,
        claimed
    );
    let cases = [
        (
            ProjectClaimRefusal::AlreadyHeld {
                holder: PeerId::try_from(ID.to_owned())?,
            },
            json!({"kind":"alreadyHeld","value":{"holder":ID}}),
        ),
        (
            ProjectClaimRefusal::TransferInProgress {
                transfer: TransferId::try_from(ID.to_owned())?,
            },
            json!({"kind":"transferInProgress","value":{"transfer":ID}}),
        ),
        (
            ProjectClaimRefusal::SchemaTooNew {
                store: SchemaIdentity::new(9, "native-fingerprint".into())?,
                supported: Some(8),
            },
            json!({"kind":"schemaTooNew","value":{"store":{"version":9,"fingerprint":"native-fingerprint"},"supported":8}}),
        ),
        (ProjectClaimRefusal::NotFound, json!({"kind":"notFound"})),
        (ProjectClaimRefusal::Retired, json!({"kind":"retired"})),
        (ProjectClaimRefusal::Archived, json!({"kind":"archived"})),
    ];
    for (reason, value) in cases {
        let reply = ProjectClaimReply::Refused(reason);
        let expected = json!({"kind":"refused","value":value});
        assert_eq!(serde_json::to_value(&reply)?, expected);
        assert_eq!(
            serde_json::from_value::<ProjectClaimReply>(expected)?,
            reply
        );
    }
    assert!(serde_json::from_value::<ProjectClaimReply>(json!({"kind":"claimed","value":{"epoch":0,"expiresAt":0,"start":{"epoch":1,"position":7}}})).is_err());
    assert!(serde_json::from_value::<ProjectClaimReply>(json!({"kind":"refused","value":{"kind":"schemaTooNew","value":{"store":{"version":9,"fingerprint":""},"supported":8}}})).is_err());
    Ok(())
}
