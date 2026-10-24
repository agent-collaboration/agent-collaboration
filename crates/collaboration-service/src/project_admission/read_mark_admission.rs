//! Covered-read updates use the library write gate and the accepted project line.
use super::{ProjectAdmission, ProjectAdmissionError};
use message_board::*;
use message_board_storage::CursorValidity;

impl ProjectAdmission {
    pub async fn mark_covered_read(
        &self,
        project: &ProjectId,
        reader: &Identity,
        scope: ReadScope,
        covered: FeedCursor,
    ) -> Result<ReadMarkOutcome, ProjectAdmissionError> {
        let Some(store) = self.registry.existing_project(project).await? else {
            return Err(ProjectAdmissionError::InvalidRecord(
                "read-mark project store missing".to_owned(),
            ));
        };
        let mut write = match store.begin_admission(self.record_origin()).await {
            Ok(write) => write,
            Err(error) => {
                if let Some(result) = super::write_gate::gate_result(&error)? {
                    return Ok(ReadMarkOutcome::NotAdvanced {
                        reason: gate_reason(result)?,
                    });
                }
                return Err(error.into());
            }
        };
        if write.cursor_validity(covered).await? != CursorValidity::Valid {
            write.rollback().await?;
            return Err(ProjectAdmissionError::InvalidRecord(
                "read-mark coverage does not belong to the current project snapshot".to_owned(),
            ));
        }
        let valid_scope = match &scope {
            ReadScope::Project { project_id } => project_id == project,
            ReadScope::Thread { thread_id } => matches!(
                write.record(&thread_id.clone().into()).await?,
                Some(Record::Thread(_))
            ),
        };
        if !valid_scope {
            write.rollback().await?;
            return Err(ProjectAdmissionError::InvalidRecord(
                "read-mark coverage does not belong to the current project snapshot".to_owned(),
            ));
        }
        let role_keys = write.stood_down_actor_role_keys(reader).await?;
        let mut role_records = write
            .record_batch(&role_keys)
            .await?
            .into_iter()
            .map(|row| row.decode())
            .collect::<Result<Vec<_>, _>>()?;
        let assignments: Vec<_> = role_records
            .iter()
            .filter_map(|record| match record {
                Record::RoleAssignment(role)
                    if role.holder.declared_identity().as_ref() == Some(reader)
                        && matches!(role.ended, Some(RoleEnd::StoodDown { .. })) =>
                {
                    Some(role.assignment_id.clone())
                }
                _ => None,
            })
            .collect();
        let acknowledgement_keys = write.stand_down_ack_keys(reader, &assignments).await?;
        for row in write.record_batch(&acknowledgement_keys).await? {
            role_records.push(row.decode()?);
        }
        super::operation_operand_reads::validate_acknowledgement_cursors(&mut write, &role_records)
            .await?;
        if super::operation_context::has_unacknowledged_stand_down(&role_records, reader) {
            write.rollback().await?;
            return Ok(ReadMarkOutcome::NotAdvanced {
                reason: ReadMarkNotAdvanced::StandDownUnacknowledged {},
            });
        }
        let now = chrono::Utc::now();
        let (current, normalized) = write.normalize_read_mark(reader, &scope, now).await?;
        if current
            .as_ref()
            .is_some_and(|mark| mark.through.position >= covered.position)
        {
            if normalized {
                write.commit().await?;
            } else {
                write.rollback().await?;
            }
            return Ok(ReadMarkOutcome::Unchanged {});
        }
        let Some(Record::Project(state)) = write.record(&project.clone().into()).await? else {
            return Err(ProjectAdmissionError::InvalidRecord(
                "read-mark project state missing".to_owned(),
            ));
        };
        let id = current.map_or_else(RecordId::generate, |mark| mark.envelope.id);
        write
            .write_reader_mark(ReadMark {
                envelope: RecordEnvelope {
                    id,
                    scope: state.envelope.scope,
                    actor: RecordedActor::declared(reader.clone()),
                    request: None,
                    position: RecordPlacement::Unsequenced,
                    time: RecordTime::Router(now),
                },
                reader: reader.clone(),
                scope,
                through: covered,
            })
            .await?;
        write.commit().await?;
        Ok(ReadMarkOutcome::Advanced { through: covered })
    }
}
fn gate_reason(result: RequestResult) -> Result<ReadMarkNotAdvanced, ProjectAdmissionError> {
    Ok(match result {
        RequestResult::NotWriter { current } => ReadMarkNotAdvanced::NotWriter { current },
        RequestResult::LeaseExpired { expired_at } => {
            ReadMarkNotAdvanced::LeaseExpired { expired_at }
        }
        RequestResult::TransferInProgress { transfer } => {
            ReadMarkNotAdvanced::TransferInProgress { transfer }
        }
        RequestResult::SchemaNotReady(refusal) => ReadMarkNotAdvanced::SchemaNotReady { refusal },
        RequestResult::Overloaded => ReadMarkNotAdvanced::Overloaded {},
        _ => {
            return Err(ProjectAdmissionError::InvalidRecord(
                "unexpected read-mark admission gate".to_owned(),
            ));
        }
    })
}
