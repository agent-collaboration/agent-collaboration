//! VW-6's unkeyed entry uses the same project lock, local lease and schema authority.
use super::{ProjectAdmission, ProjectAdmissionError, granted_epoch};
use message_board::*;
use message_board_storage::StoreSchemaState;

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
        let guard = store.lock_writer().await;
        let epoch = match granted_epoch(self.leases.grant(project).await?) {
            Ok(epoch) => epoch,
            Err(result) => {
                return Ok(ReadMarkOutcome::NotAdvanced {
                    reason: gate_reason(result)?,
                });
            }
        };
        if let StoreSchemaState::ReadOnly { refusal } = guard.schema_state() {
            return Ok(ReadMarkOutcome::NotAdvanced {
                reason: ReadMarkNotAdvanced::SchemaNotReady {
                    refusal: refusal.clone(),
                },
            });
        }
        let mut write = guard.begin().await?;
        let high_water = write.high_water().await?;
        if epoch != high_water.epoch {
            write.rollback().await?;
            return Ok(ReadMarkOutcome::NotAdvanced {
                reason: ReadMarkNotAdvanced::NotWriter {
                    current: Some(high_water.epoch),
                },
            });
        }
        if covered.epoch != epoch || covered.position > high_water.position {
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
                    if role.holder.declared_identity() == Some(reader)
                        && matches!(role.ended, Some(RoleEnd::StoodDown { .. })) =>
                {
                    match role.envelope.position {
                        RecordPosition::Project(cursor) => {
                            Some((role.assignment_id.clone(), cursor.epoch))
                        }
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect();
        let acknowledgement_keys = write.stand_down_ack_keys(reader, &assignments).await?;
        for row in write.record_batch(&acknowledgement_keys).await? {
            role_records.push(row.decode()?);
        }
        if super::operation_context::has_unacknowledged_stand_down(&role_records, reader) {
            write.rollback().await?;
            return Ok(ReadMarkOutcome::NotAdvanced {
                reason: ReadMarkNotAdvanced::StandDownUnacknowledged {},
            });
        }
        let current = write.read_mark(reader, &scope).await?;
        if current.as_ref().is_some_and(|mark| {
            mark.through.epoch == epoch && mark.through.position >= covered.position
        }) {
            write.rollback().await?;
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
                    actor: RecordedActor {
                        actor: ActorKind::Declared(reader.clone()),
                        installation: self.installation.clone(),
                        machine: self.machine.clone(),
                    },
                    request: None,
                    position: RecordPosition::Unsequenced,
                    router_time: chrono::Utc::now(),
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
        RequestResult::SchemaNotReady(refusal) => ReadMarkNotAdvanced::SchemaNotReady { refusal },
        RequestResult::Overloaded => ReadMarkNotAdvanced::Overloaded {},
        _ => {
            return Err(ProjectAdmissionError::InvalidRecord(
                "unexpected read-mark admission gate".to_owned(),
            ));
        }
    })
}
