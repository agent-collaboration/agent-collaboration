//! Bounded large Fleet records round-trip through canonical writes, reads and encoding.
#[path = "maximum_rows/definition.rs"]
mod definition;
#[path = "maximum_rows/support.rs"]
mod support;
use super::support::TestResult;
use fleet_admission::FleetOperation;
use message_board::*;
use support::{
    admit, canonical_fixture, description, envelope, import_prerequisites, measure, name, open,
    repository, workspace,
};

#[tokio::test]
async fn maximum_workspace_row_round_trips_in_the_canonical_store() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = open(root.path()).await?;
    let proof = async {
        let (workspace_id, _, _) = import_prerequisites(&fixture).await?;
        let mut latest = admit(
            &fixture,
            FleetOperation::WorkspaceUpdate {
                name: name()?,
                description: description()?,
            },
        )
        .await?;
        let repositories = (0..16)
            .map(|index| {
                Ok(RepositoryRef::Origin {
                    normalized_origin: repository(index)?,
                })
            })
            .collect::<TestResult<Vec<_>>>()?;
        for repository in &repositories {
            latest = admit(
                &fixture,
                FleetOperation::RepositoryAttach {
                    repository: repository.clone(),
                },
            )
            .await?;
        }
        let (mut request, effect) = latest;
        request.request_id = RequestId::generate();
        let expected = Record::Workspace(Workspace {
            envelope: envelope(&request, effect, None)?,
            workspace_id,
            name: name()?,
            description: description()?,
            state: WorkspaceState::Archiving,
            repositories,
        });
        measure(
            &fixture,
            "Workspace",
            canonical_fixture(&fixture, expected).await?,
        )
        .await
    }
    .await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn maximum_board_row_round_trips_in_the_canonical_store() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = open(root.path()).await?;
    let proof = async {
        let (workspace_id, board, _) = import_prerequisites(&fixture).await?;
        let kind = BoardKind::Initiative {
            goal: "\0".repeat(32 * 1024),
            done_when: "\0".repeat(32 * 1024),
        };
        let (mut request, effect) = admit(
            &fixture,
            FleetOperation::BoardUpdate {
                board: board.clone(),
                name: name()?,
                description: description()?,
            },
        )
        .await?;
        request.request_id = RequestId::generate();
        let expected = Record::Board(Board {
            envelope: envelope(&request, effect, Some(board.clone()))?,
            board_id: board,
            workspace_id,
            name: name()?,
            description: description()?,
            kind,
            state: BoardState::Archiving,
        });
        measure(
            &fixture,
            "Board",
            canonical_fixture(&fixture, expected).await?,
        )
        .await
    }
    .await;
    fixture.objects.close().await?;
    proof
}

fn small_adr(adr: AdrId, supersedes: Option<AdrId>) -> TestResult<FleetOperation> {
    Ok(FleetOperation::AdrCreate {
        adr,
        title: ResourceName::try_from(durable_steps::Uuid::now_v7().to_string())?,
        text: MarkdownText::try_from("Prerequisite".to_owned())?,
        supersedes,
        made_in: None,
        repository_files: Vec::new(),
    })
}

#[tokio::test]
async fn maximum_adr_row_round_trips_in_the_canonical_store() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = open(root.path()).await?;
    let proof = async {
        let workspace_id = workspace(&fixture).await?;
        let previous = AdrId::generate();
        admit(&fixture, small_adr(previous.clone(), None)?).await?;
        let adr = AdrId::generate();
        let made_in = RecordRef {
            kind: RecordKind::StandDownAcknowledgement,
            id: RecordId::generate(),
        };
        let artifacts = (0..16)
            .map(|index| {
                Ok(ArtifactRef {
                    repository: repository(index)?,
                    path: "\0".repeat(MAX_EVIDENCE_PATH_BYTES),
                    commit: Some(CommitSha::try_from("f".repeat(64))?),
                })
            })
            .collect::<TestResult<Vec<_>>>()?;
        let text = MarkdownText::try_from("\"".repeat(MAX_MESSAGE_TEXT_BYTES))?;
        admit(
            &fixture,
            FleetOperation::AdrCreate {
                adr: adr.clone(),
                title: name()?,
                text: text.clone(),
                supersedes: Some(previous.clone()),
                made_in: Some(made_in.clone()),
                repository_files: artifacts.clone(),
            },
        )
        .await?;
        admit(
            &fixture,
            FleetOperation::AdrTransition {
                adr: adr.clone(),
                transition: AdrTransition::Accept {},
                expected_revision: SubjectRevision::INITIAL,
            },
        )
        .await?;
        let replacement = AdrId::generate();
        admit(&fixture, small_adr(replacement.clone(), Some(adr.clone()))?).await?;
        admit(
            &fixture,
            FleetOperation::AdrTransition {
                adr: replacement.clone(),
                transition: AdrTransition::Accept {},
                expected_revision: SubjectRevision::INITIAL,
            },
        )
        .await?;
        let (request, effect) = admit(
            &fixture,
            FleetOperation::AdrTransition {
                adr: adr.clone(),
                transition: AdrTransition::Supersede { by: replacement },
                expected_revision: SubjectRevision::try_from(1)?,
            },
        )
        .await?;
        let expected = Record::Adr(Adr {
            envelope: envelope(&request, effect, None)?,
            adr_id: adr,
            workspace: workspace_id,
            title: name()?,
            text,
            status: AdrStatus::Superseded,
            supersedes: Some(previous),
            made_in: Some(made_in),
            repository_files: artifacts,
            revision: SubjectRevision::try_from(2)?,
        });
        measure(&fixture, "Adr", expected).await
    }
    .await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn maximum_process_definition_row_round_trips_in_the_canonical_store() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = open(root.path()).await?;
    let proof = async {
        workspace(&fixture).await?;
        let definition = definition::maximum_definition()?;
        let parsed = ProcessDefinitionSource::parse(&definition.source)?;
        let (request, effect) = admit(
            &fixture,
            FleetOperation::DefinitionRegister {
                definition: definition.clone(),
            },
        )
        .await?;
        let expected = Record::ProcessDefinition(ProcessDefinition {
            envelope: envelope(&request, effect, None)?,
            reference: definition.reference,
            source: definition.source,
            initial_status: parsed.initial_status,
            edges: parsed.edges,
        });
        measure(&fixture, "ProcessDefinition", expected).await
    }
    .await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn maximum_directory_entry_row_round_trips_in_the_canonical_store() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = open(root.path()).await?;
    let proof = async {
        let (_, board, project) = import_prerequisites(&fixture).await?;
        let (request, effect) = admit(
            &fixture,
            FleetOperation::ProjectDirectoryUpdate {
                project: project.clone(),
                board: board.clone(),
                title: name()?,
            },
        )
        .await?;
        support::maximum_directory_summary(&fixture, &project).await?;
        let expected = Record::DirectoryEntry(ProjectDirectoryEntry {
            envelope: envelope(&request, effect, Some(board.clone()))?,
            id: project,
            board,
            title: name()?,
        });
        measure(&fixture, "DirectoryEntry", expected).await
    }
    .await;
    fixture.objects.close().await?;
    proof
}
