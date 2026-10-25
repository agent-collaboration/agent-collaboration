//! Literal description inputs and real project creation shared by native scenarios.
use super::board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};

pub fn description_with_control_characters(version: u32) -> String {
    format!(
        "{version:08x}{}",
        "\0\u{0001}\n\t".repeat((16 * 1024 - 8) / 4)
    )
}

pub async fn create_project(
    fixture: &BoardFixture,
    description: Option<&str>,
) -> Result<(ProjectId, Value), TestError> {
    let project = ProjectId::generate();
    let mut request = fixture.header();
    request.as_object_mut().ok_or("creation header")?.extend(
        json!({"projectId":project,"boardId":fixture.board,"title":"described project"})
            .as_object()
            .ok_or("creation fields")?
            .clone(),
    );
    if let Some(description) = description {
        request
            .as_object_mut()
            .ok_or("creation object")?
            .insert("description".to_owned(), json!(description));
    }
    fixture
        .prepare_registered_creation(&serde_json::from_value(request.clone())?)
        .await?;
    accepted(&fixture.api.call("project_create", request.clone()).await?);
    Ok((project, request))
}

pub async fn project_state(
    fixture: &BoardFixture,
    project: &ProjectId,
) -> Result<ProjectState, TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project store")?;
    let mut read = store.snapshot().await?;
    let record = read.record(&project.clone().into()).await?;
    read.finish().await?;
    match record {
        Some(Record::Project(state)) => Ok(state),
        _ => Err("project state missing".into()),
    }
}
