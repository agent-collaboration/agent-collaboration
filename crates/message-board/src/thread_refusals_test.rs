use super::ThreadRefusal;
use crate::{FeedCursor, Identity, ThreadId};

fn identity(name: &str) -> Identity {
    Identity::Human {
        human_id: name.to_owned().try_into().unwrap(),
    }
}
fn thread() -> ThreadId {
    "019f0000-0000-7000-8000-00000000ab02"
        .to_owned()
        .try_into()
        .unwrap()
}
fn cursor() -> FeedCursor {
    serde_json::from_str(r#"{"epoch":2,"position":42}"#).unwrap()
}

#[test]
fn stale_seat_validation_and_serde_require_matching_holder_and_cursor_presence() {
    for holder in [None, Some(identity("current holder"))] {
        for last_seen in [None, Some(cursor())] {
            for reason in [
                ThreadRefusal::StaleOrchestrator {
                    thread_id: thread(),
                    actor: identity("actor"),
                    holder: holder.clone(),
                    last_seen,
                    named_holder: identity("former holder"),
                },
                ThreadRefusal::StaleImplementer {
                    thread_id: thread(),
                    actor: identity("actor"),
                    holder: holder.clone(),
                    last_seen,
                    named_holder: identity("former holder"),
                },
            ] {
                let expected = holder.is_some() == last_seen.is_some();
                assert_eq!(reason.validate().is_ok(), expected);
                let decoded =
                    serde_json::from_value::<ThreadRefusal>(serde_json::to_value(&reason).unwrap());
                assert_eq!(decoded.is_ok(), expected);
                if expected {
                    assert_eq!(decoded.unwrap(), reason);
                }
            }
        }
    }
}

#[test]
fn orchestrator_requirement_keeps_its_existing_independent_optional_evidence() {
    for holder in [None, Some(identity("current holder"))] {
        for last_seen in [None, Some(cursor())] {
            let reason = ThreadRefusal::OrchestratorRequired {
                thread_id: thread(),
                actor: identity("actor"),
                holder: holder.clone(),
                last_seen,
            };
            assert!(reason.validate().is_ok());
            assert_eq!(
                serde_json::from_value::<ThreadRefusal>(serde_json::to_value(&reason).unwrap())
                    .unwrap(),
                reason
            );
        }
    }
}
