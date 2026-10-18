//! Read-only checks consume one native project snapshot and the existing fleet cache.
use super::{
    ProjectAdmission, ProjectAdmissionError,
    recorded_move_check::{RecordedMoveCheck, recorded_move_verdict},
};
use message_board::*;
impl ProjectAdmission {
    pub async fn check_move(
        &self,
        request: &ProjectMoveCheckRequest,
    ) -> Result<MoveVerdict, ProjectAdmissionError> {
        let store = self
            .registry
            .existing_project(&request.project_id)
            .await?
            .ok_or_else(|| ProjectAdmissionError::InvalidRecord("project not found".to_owned()))?;
        let mut read = store.snapshot().await?;
        let Some(Record::Project(project)) =
            read.record(&request.project_id.clone().into()).await?
        else {
            return Err(ProjectAdmissionError::InvalidRecord(
                "project state is missing".to_owned(),
            ));
        };
        let mut keys = read
            .active_role_selection_keys(
                &RoleSubject::Project(request.project_id.clone()),
                AuthorityRole::Lead,
            )
            .await?;
        let mut definitions = Vec::new();
        if let Some(reference) = &project.pinned_definition
            && let Some(definition) = self.fleet.process_definition(reference).await?
        {
            if let Some(edge) = definition.edges.iter().find(|edge| {
                edge.name == request.move_name && project.status.as_ref() == Some(&edge.from)
            }) {
                keys.extend(read.guard_fact_keys(&edge.guards).await?);
            }
            definitions.push(definition);
        }
        let records: Vec<_> = read
            .record_batch(&keys)
            .await?
            .into_iter()
            .map(|row| row.decode())
            .collect::<Result<_, _>>()?;
        let archived = match &project.envelope.scope.board {
            Some(board) => matches!(self.fleet.record(&board.clone().into()).await?,
                Some(Record::Board(board)) if matches!(board.state, BoardState::Archiving | BoardState::Archived)),
            None => false,
        };
        let verdict = recorded_move_verdict(RecordedMoveCheck {
            project: &project,
            definitions: &definitions,
            records: &records,
            actor: &request.actor,
            move_name: &request.move_name,
            expected_revision: request.expected_revision,
            archived,
        })
        .map_err(|error| ProjectAdmissionError::InvalidRecord(format!("{error:?}")))?;
        read.finish().await?;
        Ok(verdict)
    }
}
