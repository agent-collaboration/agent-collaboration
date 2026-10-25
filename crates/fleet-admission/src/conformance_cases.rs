//! Engine-independent decision tables consumed by both native and central adapters.
use crate::{ArchiveReport, ArchiveReportOutcome, FleetOperation, FleetRequest};
use message_board::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FleetExpectation {
    Accepted(ChangeOutcome),
    Rejected(Refusal),
    Replay { step: usize },
    Conflicting,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FleetRecordExpectation {
    BoardState {
        board: BoardId,
        state: BoardState,
    },
    AdrState {
        adr: AdrId,
        status: AdrStatus,
        text: MarkdownText,
    },
}

#[derive(Clone, Debug)]
pub enum FleetCaseAction {
    Request {
        request: Box<FleetRequest>,
        expectation: FleetExpectation,
    },
    ArchiveReport {
        report: ArchiveReport,
        expected: ArchiveReportOutcome,
    },
}

#[derive(Clone, Debug)]
pub struct FleetCaseStep {
    pub action: FleetCaseAction,
    pub record: Option<FleetRecordExpectation>,
    pub reopen_after: bool,
}

#[derive(Clone, Debug)]
pub struct FleetCase {
    pub name: &'static str,
    pub steps: Vec<FleetCaseStep>,
}

type FixtureError = Box<dyn std::error::Error + Send + Sync>;

fn id(ordinal: usize) -> String {
    format!("01890f2e-7b4c-7cc0-98c4-{ordinal:012x}")
}
fn request(
    workspace: &WorkspaceId,
    ordinal: usize,
    operation: FleetOperation,
) -> Result<FleetRequest, FixtureError> {
    Ok(FleetRequest {
        request_id: RequestId::try_from(id(100 + ordinal))?,
        workspace: workspace.clone(),
        actor: RecordedActor::declared(Identity::Human {
            human_id: HumanId::try_from("owner".to_owned())?,
        }),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}
fn step(request: FleetRequest, expectation: FleetExpectation) -> FleetCaseStep {
    FleetCaseStep {
        action: FleetCaseAction::Request {
            request: Box::new(request),
            expectation,
        },
        record: None,
        reopen_after: false,
    }
}
fn report_step(
    board: &BoardId,
    project: ProjectId,
    expected: ArchiveReportOutcome,
) -> Result<FleetCaseStep, FixtureError> {
    Ok(FleetCaseStep {
        action: FleetCaseAction::ArchiveReport {
            report: ArchiveReport {
                board: board.clone(),
                project,
                position: fleet_sync_protocol::ReplicationPosition::new(
                    fleet_sync_protocol::Epoch::new(1)?,
                    fleet_sync_protocol::CommitSequence::new(7),
                ),
                installation: ServiceId::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )?,
                reported_at: "2026-10-12T00:00:00Z".parse()?,
            },
            expected,
        },
        record: None,
        reopen_after: false,
    })
}
fn name(value: &str) -> Result<ResourceName, FixtureError> {
    Ok(ResourceName::try_from(value.to_owned())?)
}
fn description() -> Result<Description, FixtureError> {
    Ok(Description::try_from("conformance".to_owned())?)
}
fn workspace_create() -> Result<FleetOperation, FixtureError> {
    Ok(FleetOperation::WorkspaceCreate {
        name: name("Router")?,
        description: description()?,
    })
}

pub fn cases() -> Result<Vec<FleetCase>, FixtureError> {
    let workspace = WorkspaceId::try_from(id(1))?;
    let board = BoardId::try_from(id(2))?;
    let project = ProjectId::try_from(id(3))?;
    let other_project = ProjectId::try_from(id(4))?;
    let adr = AdrId::try_from(id(5))?;
    let successor = AdrId::try_from(id(6))?;
    let mut all = Vec::new();

    let first = request(&workspace, 0, workspace_create()?)?;
    let mut conflict = first.clone();
    conflict.operation = FleetOperation::WorkspaceUpdate {
        name: name("different")?,
        description: description()?,
    };
    let other_workspace = WorkspaceId::try_from(id(7))?;
    all.push(FleetCase {
        name: "identity-replay-and-uniqueness",
        steps: vec![
            step(
                first.clone(),
                FleetExpectation::Accepted(ChangeOutcome::Changed),
            ),
            step(first, FleetExpectation::Replay { step: 0 }),
            step(conflict, FleetExpectation::Conflicting),
            step(
                request(&workspace, 1, workspace_create()?)?,
                FleetExpectation::Rejected(Refusal::AlreadyExists(workspace.clone().into())),
            ),
            step(
                request(&other_workspace, 2, workspace_create()?)?,
                FleetExpectation::Rejected(Refusal::NameConflict {
                    name: name("Router")?,
                    existing: workspace.clone().into(),
                }),
            ),
        ],
    });

    let create_board = FleetOperation::BoardCreate {
        board: board.clone(),
        name: name("Work")?,
        description: description()?,
        board_kind: BoardKind::Area {},
    };
    let directory =
        |project: ProjectId, title: ResourceName| FleetOperation::ProjectDirectoryCreate {
            project,
            board: board.clone(),
            title,
        };
    let mut archive = vec![
        step(
            request(&workspace, 0, workspace_create()?)?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
        step(
            request(&workspace, 1, create_board)?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
        step(
            request(
                &workspace,
                2,
                FleetOperation::BoardUpdate {
                    board: board.clone(),
                    name: name("Work")?,
                    description: description()?,
                },
            )?,
            FleetExpectation::Accepted(ChangeOutcome::AlreadySo),
        ),
        step(
            request(&workspace, 3, directory(project.clone(), name("First")?))?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
        step(
            request(
                &workspace,
                4,
                directory(other_project.clone(), name("First")?),
            )?,
            FleetExpectation::Rejected(Refusal::NameConflict {
                name: name("First")?,
                existing: project.clone().into(),
            }),
        ),
        step(
            request(
                &workspace,
                5,
                directory(other_project.clone(), name("Second")?),
            )?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
        step(
            request(
                &workspace,
                6,
                FleetOperation::BoardArchive {
                    board: board.clone(),
                },
            )?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
        report_step(&board, project.clone(), ArchiveReportOutcome::Recorded)?,
        report_step(&board, project, ArchiveReportOutcome::AlreadyRecorded)?,
        report_step(&board, other_project, ArchiveReportOutcome::Recorded)?,
        step(
            request(
                &workspace,
                10,
                FleetOperation::BoardUpdate {
                    board: board.clone(),
                    name: name("after archive")?,
                    description: description()?,
                },
            )?,
            FleetExpectation::Rejected(Refusal::Archived),
        ),
    ];
    for index in [6, 7, 8] {
        if let Some(step) = archive.get_mut(index) {
            step.record = Some(FleetRecordExpectation::BoardState {
                board: board.clone(),
                state: BoardState::Archiving,
            });
        }
    }
    if let Some(step) = archive.get_mut(6) {
        step.reopen_after = true;
    }
    if let Some(step) = archive.get_mut(9) {
        step.record = Some(FleetRecordExpectation::BoardState {
            board: board.clone(),
            state: BoardState::Archived,
        });
    }
    all.push(FleetCase {
        name: "metadata-and-archive-reports",
        steps: archive,
    });

    let text =
        MarkdownText::try_from("# Immutable original\n\nThis decision stays written.".to_owned())?;
    let replacement_text = MarkdownText::try_from("# Replacement decision".to_owned())?;
    let mut adr_steps = vec![
        step(
            request(&workspace, 0, workspace_create()?)?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
        step(
            request(
                &workspace,
                1,
                FleetOperation::AdrCreate {
                    adr: adr.clone(),
                    title: name("Original")?,
                    text: text.clone(),
                    supersedes: None,
                    made_in: None,
                    repository_files: Vec::new(),
                },
            )?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
        step(
            request(
                &workspace,
                2,
                FleetOperation::AdrTransition {
                    adr: adr.clone(),
                    transition: AdrTransition::Accept {},
                    expected_revision: SubjectRevision::INITIAL,
                },
            )?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
        step(
            request(
                &workspace,
                3,
                FleetOperation::AdrTransition {
                    adr: adr.clone(),
                    transition: AdrTransition::Withdraw {},
                    expected_revision: SubjectRevision::INITIAL,
                },
            )?,
            FleetExpectation::Rejected(Refusal::StaleRevision {
                current: SubjectRevision::try_from(1)?,
            }),
        ),
        step(
            request(
                &workspace,
                4,
                FleetOperation::AdrTransition {
                    adr: adr.clone(),
                    transition: AdrTransition::Withdraw {},
                    expected_revision: SubjectRevision::try_from(1)?,
                },
            )?,
            FleetExpectation::Rejected(Refusal::Closed {
                subject: adr.clone().into(),
            }),
        ),
        step(
            request(
                &workspace,
                5,
                FleetOperation::AdrCreate {
                    adr: successor.clone(),
                    title: name("Replacement")?,
                    text: replacement_text,
                    supersedes: Some(adr.clone()),
                    made_in: None,
                    repository_files: Vec::new(),
                },
            )?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
        step(
            request(
                &workspace,
                6,
                FleetOperation::AdrTransition {
                    adr: successor.clone(),
                    transition: AdrTransition::Accept {},
                    expected_revision: SubjectRevision::INITIAL,
                },
            )?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
        step(
            request(
                &workspace,
                7,
                FleetOperation::AdrTransition {
                    adr: adr.clone(),
                    transition: AdrTransition::Supersede { by: successor },
                    expected_revision: SubjectRevision::try_from(1)?,
                },
            )?,
            FleetExpectation::Accepted(ChangeOutcome::Changed),
        ),
    ];
    if let Some(step) = adr_steps.get_mut(2) {
        step.record = Some(FleetRecordExpectation::AdrState {
            adr: adr.clone(),
            status: AdrStatus::Accepted,
            text: text.clone(),
        });
    }
    if let Some(step) = adr_steps.get_mut(7) {
        step.record = Some(FleetRecordExpectation::AdrState {
            adr,
            status: AdrStatus::Superseded,
            text,
        });
    }
    all.push(FleetCase {
        name: "adr-content-and-lifecycle",
        steps: adr_steps,
    });

    Ok(all)
}
