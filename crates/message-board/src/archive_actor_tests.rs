//! Decision1200: internal archive provenance has one strict camelCase wire identity.
use crate::{ActorKind, Identity, RecordedActor, RouterComponent, ServiceId};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
#[allow(clippy::panic_in_result_fn)]
fn archive_observation_actor_round_trips_without_becoming_a_declared_identity() -> TestResult {
    // Wrong build: new stored provenance is missing, renamed, or accepted as public identity.
    let installation = ServiceId::try_from("019f0000-0000-7000-8000-000000001200".to_owned())?;
    let wire =
        json!({"kind":"router","component":"archiveObservation","installation":installation});
    let actor: RecordedActor = serde_json::from_value(wire.clone())?;
    assert_eq!(
        actor,
        RecordedActor::Router {
            component: RouterComponent::ArchiveObservation,
            installation,
        }
    );
    assert_eq!(serde_json::to_value(&actor)?, wire);
    assert_eq!(
        actor.kind(),
        ActorKind::Router {
            component: RouterComponent::ArchiveObservation
        }
    );
    assert!(actor.declared_identity().is_none());
    assert!(serde_json::from_value::<Identity>(wire).is_err());
    Ok(())
}
