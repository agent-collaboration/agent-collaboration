#![allow(clippy::unwrap_used)] // Test-only fixture failures must fail the test.

use message_board::*;

pub fn record_envelope(id: RecordId, project: ProjectId) -> RecordEnvelope {
    RecordEnvelope {
        id,
        scope: RecordScope {
            workspace: WorkspaceId::try_from("01890f2e-7b4c-7cc0-98c4-000000000100".to_owned())
                .unwrap(),
            board: Some(
                BoardId::try_from("01890f2e-7b4c-7cc0-98c4-000000000101".to_owned()).unwrap(),
            ),
            project: Some(project),
        },
        actor: RecordedActor {
            actor: ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("writer".to_owned()).unwrap(),
            }),
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())
                .unwrap(),
            machine: MachineLabel::try_from("test machine".to_owned()).unwrap(),
        },
        request: Some(RequestId::generate()),
        position: RecordPosition::Project(FeedCursor {
            epoch: EpochNumber::try_from(1).unwrap(),
            position: FeedPosition::try_from(1).unwrap(),
        }),
        router_time: "2026-10-12T00:00:00Z".parse().unwrap(),
    }
}
