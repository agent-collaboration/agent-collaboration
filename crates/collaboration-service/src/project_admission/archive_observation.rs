//! A holder records one internal archive fact under its ordinary native project lock.
use super::{ProjectAdmission, ProjectAdmissionError};
use chrono::Utc;
use collaboration_runtime::NativeProjectStore;
use message_board::{
    BoardId, BoardState, Fact, FactId, FactKind, FeedCursor, LifecycleFact, ProjectId, Record,
    RecordEnvelope, RecordKind, RecordTime, RecordedActor, RouterComponent, ServiceId,
};
use std::sync::Arc;

pub struct ProjectArchiveObservation {
    /// The physical native owner that admitted or retained this fact.
    pub store: Arc<NativeProjectStore>,
    pub board: BoardId,
    pub installation: ServiceId,
    pub position: FeedCursor,
}

impl ProjectAdmission {
    /// Only the Host's authoritative Fleet reconciliation calls this internal admission path.
    pub async fn observe_archiving_project(
        &self,
        project: &ProjectId,
    ) -> Result<Option<ProjectArchiveObservation>, ProjectAdmissionError> {
        let Some(store) = self.registry.existing_project(project).await? else {
            return Ok(None);
        };
        let mut write = store.begin_admission(self.record_origin()).await?;
        let Some(Record::Project(state)) = write.record(&project.clone().into()).await? else {
            return Err(ProjectAdmissionError::InvalidRecord(
                "project state is missing".into(),
            ));
        };
        let Some(board) = state.envelope.scope.board.clone() else {
            write.rollback().await?;
            return Ok(None);
        };
        if !matches!(self.fleet.record(&board.clone().into()).await?,
            Some(Record::Board(value)) if value.state == BoardState::Archiving)
        {
            write.rollback().await?;
            return Ok(None);
        }
        let actor = RecordedActor::Router {
            component: RouterComponent::ArchiveObservation,
            installation: self.installation.clone(),
        };
        for record in write.records(RecordKind::Fact).await? {
            if let Record::Fact(fact) = record
                && fact.envelope.actor == actor
                && matches!(&fact.kind, FactKind::Lifecycle(LifecycleFact::ArchiveSeen { board: observed }) if observed == &board)
            {
                let position = fact.envelope.position.project_cursor().ok_or_else(|| {
                    ProjectAdmissionError::InvalidRecord(
                        "archive fact has no project position".into(),
                    )
                })?;
                write.rollback().await?;
                return Ok(Some(ProjectArchiveObservation {
                    store,
                    board,
                    installation: self.installation.clone(),
                    position,
                }));
            }
        }
        let high_water = write.high_water().await?;
        let position = FeedCursor {
            epoch: write.admitted_epoch()?,
            position: high_water.position.successor()?,
        };
        let fact_id = FactId::generate();
        let now = Utc::now();
        write
            .append_record(Record::Fact(Fact {
                envelope: RecordEnvelope {
                    id: fact_id.clone().into(),
                    scope: state.envelope.scope,
                    actor,
                    request: None,
                    position: write.record_placement(position),
                    time: RecordTime::Router(now),
                },
                fact_id,
                kind: FactKind::Lifecycle(LifecycleFact::ArchiveSeen {
                    board: board.clone(),
                }),
                links: vec![project.clone().into()],
                producer_time: now,
                anchor: Some(high_water),
                late: false,
            }))
            .await?;
        write.commit().await?;
        self.registry.notify_activity(project);
        Ok(Some(ProjectArchiveObservation {
            store,
            board,
            installation: self.installation.clone(),
            position,
        }))
    }
}
