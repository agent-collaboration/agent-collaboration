#![allow(dead_code)]
use super::registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::{FleetPayload, FleetRequestEnvelope, SessionAnswer};
use fleet_admission::*;
use message_board::*;
use registry_support::{at, identity, machine, position, start};

pub struct ArchiveFixture {
    pub store: registry_support::sql_support::SqlTxFake,
    pub workspace: WorkspaceId,
    pub board: BoardId,
    pub projects: [ProjectId; 2],
    pub actor: RecordedActor,
}

impl ArchiveFixture {
    pub fn new() -> Self {
        let workspace = WorkspaceId::try_from(identity(801)).expect("workspace");
        let board = BoardId::try_from(identity(802)).expect("board");
        let projects =
            [803, 804].map(|number| ProjectId::try_from(identity(number)).expect("project"));
        let actor = RecordedActor {
            actor: ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("archive owner".to_owned()).expect("owner"),
            }),
            installation: ServiceId::try_from(machine(1).as_str().to_owned())
                .expect("installation"),
            machine: MachineLabel::try_from("archive source".to_owned()).expect("machine"),
        };
        let mut fixture = Self {
            store: registry_support::sql_support::SqlTxFake::blank(),
            workspace: workspace.clone(),
            board: board.clone(),
            projects: projects.clone(),
            actor: actor.clone(),
        };
        let mut records = vec![
            FleetImportRecord::Workspace {
                id: workspace,
                name: ResourceName::try_from("Archive Workspace".to_owned()).expect("name"),
                description: Description::try_from("Archive description".to_owned())
                    .expect("description"),
                repositories: vec![],
                created_at: start().0,
            },
            FleetImportRecord::Board {
                id: board.clone(),
                workspace: fixture.workspace.clone(),
                name: ResourceName::try_from("Archive Board".to_owned()).expect("name"),
                description: Description::try_from("Archive description".to_owned())
                    .expect("description"),
                state: ImportedBoardState::Active,
                created_at: start().0,
            },
        ];
        records.extend(projects.into_iter().enumerate().map(|(index, project)| {
            FleetImportRecord::Directory {
                project,
                board: board.clone(),
                title: ResourceName::try_from(format!("Project {index}")).expect("title"),
                created_at: start().0,
            }
        }));
        assert!(matches!(
            fixture.call(
                FleetCall::Import {
                    bundle: Box::new(FleetImportBundle {
                        provenance: ImportProvenance {
                            installation: actor.installation,
                            machine: actor.machine,
                        },
                        records,
                    }),
                },
                0,
            ),
            FleetResult::Imported(FleetImportResult::Imported { .. })
        ));
        fixture
    }

    pub fn call(&mut self, call: FleetCall, minute: i64) -> FleetResult {
        let effects = self
            .store
            .run(
                WireInput::Call {
                    from: ObjectAddress::Connection(machine(1)),
                    call_id: ObjectCallId::new(identity(890)).expect("call"),
                    call: Box::new(WireCall::Fleet(call)),
                },
                at(minute),
            )
            .expect("real Fleet transaction must answer the supported call");
        effects
            .into_iter()
            .find_map(|effect| match effect {
                WireEffect::Return {
                    result: WireResult::Fleet(result),
                    ..
                } => Some(result),
                _ => None,
            })
            .expect("typed Fleet result")
    }

    pub fn archive_workspace(&mut self) {
        let request = FleetRequest {
            request_id: RequestId::try_from(identity(891)).expect("request"),
            workspace: self.workspace.clone(),
            actor: self.actor.clone(),
            acting_for: None,
            resubmits: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation: FleetOperation::WorkspaceArchive {},
        };
        let FleetResult::Fleet(SessionAnswer::FleetDecision { decision, .. }) = self.call(
            FleetCall::Fleet {
                installation: machine(1),
                request: FleetRequestEnvelope {
                    request_id: collaboration_replication_protocol::RequestId::new(
                        request.request_id.as_str(),
                    )
                    .expect("request"),
                    operation: FleetPayload::new(serde_json::to_value(request).expect("request")),
                },
            },
            1,
        ) else {
            panic!("archive request answer");
        };
        assert!(matches!(
            serde_json::from_value::<FleetRequestResult>(decision.as_value().clone())
                .expect("decision"),
            FleetRequestResult::Decided(RequestDecision::Accepted { .. })
        ));
        assert!(
            matches!(self.board_record(), Record::Board(value) if value.state == BoardState::Archiving)
        );
    }

    pub fn report(&mut self, index: usize, reporter: u64, minute: i64) -> SessionAnswer {
        match self.call(
            FleetCall::ArchiveSeen {
                installation: machine(reporter),
                board: collaboration_replication_protocol::BoardId::new(self.board.as_str())
                    .expect("board"),
                project: collaboration_replication_protocol::ProjectId::new(
                    self.projects
                        .get(index)
                        .expect("report fixture project")
                        .as_str(),
                )
                .expect("project"),
                position: position(2, 7),
            },
            minute,
        ) {
            FleetResult::ArchiveSeen(answer) => answer,
            _ => panic!("automated report answer"),
        }
    }

    pub fn record(&self, kind: &str, id: &str) -> Record {
        let payload: String = self
            .store
            .connection
            .query_row(
                "SELECT payload FROM fleet_records WHERE kind=? AND id=?",
                [kind, id],
                |row| row.get(0),
            )
            .expect("actual Fleet record");
        serde_json::from_str(&payload).expect("record")
    }

    pub fn board_record(&self) -> Record {
        self.record("board", self.board.as_str())
    }

    pub fn ledger(&self) -> Vec<String> {
        self.store
            .connection
            .prepare("SELECT payload FROM fleet_request_ledger ORDER BY request_id")
            .expect("ledger")
            .query_map([], |row| row.get(0))
            .expect("rows")
            .collect::<rusqlite::Result<_>>()
            .expect("ledger contents")
    }

    pub fn feed(&self) -> Vec<FleetFeedEntry> {
        self.store
            .connection
            .prepare("SELECT entry FROM fleet_feed ORDER BY position")
            .expect("feed")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("rows")
            .map(|row| serde_json::from_str(&row.expect("row")).expect("entry"))
            .collect()
    }
}

pub fn assert_report_answer(answer: SessionAnswer, outcome: &str) {
    // This literal is the reviewed public contract. It also compiles against
    // the old protocol, so missing dispatch is an executable behavioral RED.
    assert_eq!(
        serde_json::to_value(answer).expect("wire answer"),
        serde_json::json!({"kind":"archiveReport", "value":{"kind":outcome}})
    );
}

pub fn reservation_call(kind: &str, project: &ProjectId, attempt: u64) -> FleetCall {
    serde_json::from_value(serde_json::json!({
        "kind":kind,
        "value":{"project":project.as_str(), "attempt":identity(attempt)}
    }))
    .expect("reviewed reservation call must decode at the real boundary")
}

pub fn reservation_result(result: FleetResult) -> serde_json::Value {
    serde_json::to_value(result).expect("reservation result")
}
