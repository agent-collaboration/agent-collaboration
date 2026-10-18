//! Normalized relational records use explicit insert or update under the project writer.
use crate::project_record_io::record_kind_tag;
use crate::{ProjectStorageError, ProjectWriteTransaction};
use message_board::Record;
impl ProjectWriteTransaction {
    pub(crate) async fn write_record_relations(
        &mut self,
        record: &Record,
    ) -> Result<(), ProjectStorageError> {
        match record {
            Record::Project(row) => {
                let id = row.project_id.as_str();
                sqlx_turso::query!("INSERT INTO project_state(project_id) VALUES (?) ON CONFLICT(project_id) DO NOTHING", id).execute(&mut *self.transaction).await?;
            }
            Record::Workstream(row) => {
                let id = row.workstream_id.as_str();
                let project = row.project_id.as_str();
                sqlx_turso::query!("INSERT INTO workstreams(workstream_id, project_id) VALUES (?, ?) ON CONFLICT(workstream_id) DO NOTHING", id, project).execute(&mut *self.transaction).await?;
            }
            Record::Task(row) => {
                let id = row.task_id.as_str();
                let workstream = row.workstream_id.as_str();
                let row_count = sqlx_turso::query_scalar!(
                    r#"SELECT COUNT(*) AS "row_count!: i64" FROM tasks WHERE task_id = ?"#,
                    id
                )
                .fetch_one(&mut *self.transaction)
                .await?;
                let changed = match row_count {
                    0 => {
                        sqlx_turso::query!(
                            "INSERT INTO tasks(task_id, workstream_id) VALUES (?, ?)",
                            id,
                            workstream
                        )
                        .execute(&mut *self.transaction)
                        .await?
                    }
                    1 => {
                        sqlx_turso::query!(
                            "UPDATE tasks SET workstream_id = ? WHERE task_id = ?",
                            workstream,
                            id
                        )
                        .execute(&mut *self.transaction)
                        .await?
                    }
                    _ => return Err(ProjectStorageError::InvalidFeed),
                };
                if changed.rows_affected() != 1 {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: record.reference(),
                        field: "normalizedRelation",
                    });
                }
            }
            Record::Subtask(row) => {
                let id = row.subtask_id.as_str();
                let task = row.task_id.as_str();
                let place = String::from(row.place.clone());
                let checked = row.checked;
                let row_count = sqlx_turso::query_scalar!(
                    r#"SELECT COUNT(*) AS "row_count!: i64" FROM subtasks WHERE subtask_id = ?"#,
                    id
                )
                .fetch_one(&mut *self.transaction)
                .await?;
                let changed = match row_count {
                    0 => sqlx_turso::query!("INSERT INTO subtasks(subtask_id, task_id, place, checked) VALUES (?, ?, ?, ?)", id, task, place, checked).execute(&mut *self.transaction).await?,
                    1 => sqlx_turso::query!("UPDATE subtasks SET place = ?, checked = ? WHERE subtask_id = ?", place, checked, id).execute(&mut *self.transaction).await?,
                    _ => return Err(ProjectStorageError::InvalidFeed),
                };
                if changed.rows_affected() != 1 {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: record.reference(),
                        field: "normalizedRelation",
                    });
                }
            }
            Record::WorkTrailRow(row) => {
                let id = row.row_id.as_str();
                let redacted = row.redacted;
                sqlx_turso::query!(
                    "INSERT INTO work_trail_rows(row_id, redacted) VALUES (?, ?)",
                    id,
                    redacted
                )
                .execute(&mut *self.transaction)
                .await?;
                for (place, tag) in row.tags.iter().enumerate() {
                    let place =
                        i64::try_from(place).map_err(|_| ProjectStorageError::InvalidFeed)?;
                    let tag = tag.as_str();
                    sqlx_turso::query!(
                        "INSERT INTO work_trail_tags(row_id, place, tag) VALUES (?, ?, ?)",
                        id,
                        place,
                        tag
                    )
                    .execute(&mut *self.transaction)
                    .await?;
                }
            }
            Record::TrailRedaction(row) => {
                let id = row.row.as_str();
                let redacted = row.redacted;
                let changed = sqlx_turso::query!(
                    "UPDATE work_trail_rows SET redacted = ? WHERE row_id = ?",
                    redacted,
                    id
                )
                .execute(&mut *self.transaction)
                .await?;
                if changed.rows_affected() != 1 {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: row.row.clone().into(),
                        field: "row",
                    });
                }
            }
            Record::RoleAssignment(row) => {
                let id = row.assignment_id.as_str();
                let (project, workstream) = match row.subject() {
                    message_board::RoleSubject::Project(project) => {
                        (Some(String::from(project)), None)
                    }
                    message_board::RoleSubject::Workstream(workstream) => {
                        (None, Some(String::from(workstream)))
                    }
                };
                let role = row.role().as_str();
                let active = row.ended.is_none();
                let owner_grant = row.owner_grant.as_ref().map(|grant| grant.as_str());
                let row_count = sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "row_count!: i64" FROM role_assignments WHERE assignment_id = ?"#, id).fetch_one(&mut *self.transaction).await?;
                let changed = match row_count {
                    0 => sqlx_turso::query!("INSERT INTO role_assignments(assignment_id, project_subject, workstream_subject, role, active, owner_grant) VALUES (?, ?, ?, ?, ?, ?)", id, project, workstream, role, active, owner_grant).execute(&mut *self.transaction).await?,
                    1 => sqlx_turso::query!("UPDATE role_assignments SET active = ? WHERE assignment_id = ?", active, id).execute(&mut *self.transaction).await?,
                    _ => return Err(ProjectStorageError::InvalidFeed),
                };
                if changed.rows_affected() != 1 {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: record.reference(),
                        field: "normalizedRelation",
                    });
                }
            }
            Record::Thread(row) => {
                let id = row.thread_id.as_str();
                sqlx_turso::query!(
                    "INSERT INTO threads(thread_id) VALUES (?) ON CONFLICT(thread_id) DO NOTHING",
                    id
                )
                .execute(&mut *self.transaction)
                .await?;
                let (kind, subject) = match &row.about {
                    message_board::ThreadSubject::Project { .. } => ("project", None),
                    message_board::ThreadSubject::Workstream(subject) => {
                        ("workstream", Some(subject.as_str()))
                    }
                    message_board::ThreadSubject::Task(subject) => ("task", Some(subject.as_str())),
                    message_board::ThreadSubject::WorkTrailRow(subject) => {
                        ("workTrailRow", Some(subject.as_str()))
                    }
                    message_board::ThreadSubject::WorkspaceAdr(subject) => {
                        ("adr", Some(subject.as_str()))
                    }
                };
                let row_count = sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "row_count!: i64" FROM thread_subjects WHERE thread_id = ?"#, id).fetch_one(&mut *self.transaction).await?;
                let changed = match row_count {
                    0 => sqlx_turso::query!("INSERT INTO thread_subjects(thread_id, subject_kind, subject_id) VALUES (?, ?, ?)", id, kind, subject).execute(&mut *self.transaction).await?,
                    1 => sqlx_turso::query!("UPDATE thread_subjects SET subject_kind = ?, subject_id = ? WHERE thread_id = ?", kind, subject, id).execute(&mut *self.transaction).await?,
                    _ => return Err(ProjectStorageError::InvalidFeed),
                };
                if changed.rows_affected() != 1 {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: record.reference(),
                        field: "normalizedRelation",
                    });
                }
            }
            Record::Post(row) => {
                let id = row.post_id.as_str();
                let thread = match &row.placement {
                    message_board::Placement::Thread { thread_id } => thread_id.clone(),
                    message_board::Placement::Project { .. } => row.post_id.clone().into(),
                };
                let thread = thread.as_str();
                sqlx_turso::query!(
                    "INSERT INTO posts(post_id, thread_id) VALUES (?, ?)",
                    id,
                    thread
                )
                .execute(&mut *self.transaction)
                .await?;
            }
            Record::Request(row) => {
                let id = row.request_id.as_str();
                let hash = row.canonical_hash.as_str();
                let observed = row
                    .observed_epoch
                    .map(|epoch| epoch.get())
                    .map(i64::try_from)
                    .transpose()
                    .map_err(|_| ProjectStorageError::InvalidFeed)?;
                let accepted = row
                    .accepted_in
                    .map(|epoch| epoch.get())
                    .map(i64::try_from)
                    .transpose()
                    .map_err(|_| ProjectStorageError::InvalidFeed)?;
                let decision = serde_json::to_string(&row.decision)?;
                let resubmits = row.resubmits.as_ref().map(|request| request.as_str());
                let row_count = sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "row_count!: i64" FROM request_ledger WHERE request_id = ?"#, id).fetch_one(&mut *self.transaction).await?;
                let changed = match row_count {
                    0 => sqlx_turso::query!("INSERT INTO request_ledger(request_id, canonical_hash, observed_epoch, accepted_in, decision, resubmits) VALUES (?, ?, ?, ?, ?, ?)", id, hash, observed, accepted, decision, resubmits).execute(&mut *self.transaction).await?,
                    1 => sqlx_turso::query!("UPDATE request_ledger SET decision = ? WHERE request_id = ?", decision, id).execute(&mut *self.transaction).await?,
                    _ => return Err(ProjectStorageError::InvalidFeed),
                };
                if changed.rows_affected() != 1 {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: record.reference(),
                        field: "normalizedRelation",
                    });
                }
            }
            Record::ReadMark(row) => {
                let reader = serde_json::to_string(&row.reader)?;
                let scope = serde_json::to_string(&row.scope)?;
                let epoch = i64::try_from(row.through.epoch.get())
                    .map_err(|_| ProjectStorageError::InvalidFeed)?;
                let position = i64::try_from(row.through.position.get())
                    .map_err(|_| ProjectStorageError::InvalidFeed)?;
                let id = row.envelope.id.as_str();
                let row_count = sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "row_count!: i64" FROM read_marks WHERE reader = ? AND scope = ?"#, reader, scope).fetch_one(&mut *self.transaction).await?;
                let changed = match row_count {
                    0 => sqlx_turso::query!("INSERT INTO read_marks(reader, scope, through_epoch, through_position, record_id) VALUES (?, ?, ?, ?, ?)", reader, scope, epoch, position, id).execute(&mut *self.transaction).await?,
                    1 => sqlx_turso::query!("UPDATE read_marks SET through_epoch = ?, through_position = ?, record_id = ? WHERE reader = ? AND scope = ?", epoch, position, id, reader, scope).execute(&mut *self.transaction).await?,
                    _ => return Err(ProjectStorageError::InvalidFeed),
                };
                if changed.rows_affected() != 1 {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: record.reference(),
                        field: "normalizedRelation",
                    });
                }
            }
            Record::NotifyIntent(row) => {
                let id = row.intent_id.as_str();
                let cause_kind = record_kind_tag(row.cause.kind);
                let cause_id = row.cause.id.as_str();
                let recipient = serde_json::to_string(row.recipient())?;
                let revision = match row.recipient() {
                    message_board::IntentRecipient::Resolved(recipient) => recipient
                        .via_role
                        .as_ref()
                        .map(|role| role.assignment_revision),
                    message_board::IntentRecipient::NoHolder {
                        assignment_revision,
                        ..
                    } => Some(*assignment_revision),
                }
                .map(|revision| revision.get())
                .map(i64::try_from)
                .transpose()
                .map_err(|_| ProjectStorageError::InvalidFeed)?;
                let state = match row.state() {
                    message_board::IntentState::Emitted {} => "emitted",
                    message_board::IntentState::HandedOff {} => "handedOff",
                    message_board::IntentState::Superseded {} => "superseded",
                    message_board::IntentState::Refused { .. } => "refused",
                };
                let row_count = sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "row_count!: i64" FROM notify_intents WHERE intent_id = ?"#, id).fetch_one(&mut *self.transaction).await?;
                let changed = match row_count {
                    0 => sqlx_turso::query!("INSERT INTO notify_intents(intent_id, cause_kind, cause_id, recipient, assignment_revision, state) VALUES (?, ?, ?, ?, ?, ?)", id, cause_kind, cause_id, recipient, revision, state).execute(&mut *self.transaction).await?,
                    1 => sqlx_turso::query!("UPDATE notify_intents SET state = ?, recipient = ?, assignment_revision = ? WHERE intent_id = ?", state, recipient, revision, id).execute(&mut *self.transaction).await?,
                    _ => return Err(ProjectStorageError::InvalidFeed),
                };
                if changed.rows_affected() != 1 {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: record.reference(),
                        field: "normalizedRelation",
                    });
                }
            }
            Record::SubscriptionMembership(row) => {
                let reader = serde_json::to_string(row.reader())?;
                let scope = serde_json::to_string(row.scope())?;
                let kind = "subscriptionMembership";
                let id = row.envelope.id.as_str();
                let thread = row.scope().thread_id().map(|thread| thread.as_str());
                let row_count = sqlx_turso::query_scalar!(r#"SELECT COUNT(*) AS "row_count!: i64" FROM thread_subscriptions WHERE reader = ? AND scope = ?"#, reader, scope).fetch_one(&mut *self.transaction).await?;
                let changed = match row_count {
                    0 => sqlx_turso::query!("INSERT INTO thread_subscriptions(reader, scope, membership_kind, membership_id, thread_id) VALUES (?, ?, ?, ?, ?)", reader, scope, kind, id, thread).execute(&mut *self.transaction).await?,
                    1 => sqlx_turso::query!("UPDATE thread_subscriptions SET membership_kind = ?, membership_id = ?, thread_id = ? WHERE reader = ? AND scope = ?", kind, id, thread, reader, scope).execute(&mut *self.transaction).await?,
                    _ => return Err(ProjectStorageError::InvalidFeed),
                };
                if changed.rows_affected() != 1 {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: record.reference(),
                        field: "normalizedRelation",
                    });
                }
            }
            Record::OwnerGrant(row) => {
                let id = row.id.as_str();
                let message_board::GrantKind::ReplaceLead { project, .. } = &row.grant;
                let project = project.as_str();
                sqlx_turso::query!(
                    "INSERT INTO owner_grants(grant_id, project_id) VALUES (?, ?)",
                    id,
                    project
                )
                .execute(&mut *self.transaction)
                .await?;
            }
            Record::StandDownAcknowledgement(row) => {
                let id = row.envelope.id.as_str();
                let assignment = row.assignment.as_str();
                let epoch = i64::try_from(row.read_through.epoch.get())
                    .map_err(|_| ProjectStorageError::InvalidFeed)?;
                let position = i64::try_from(row.read_through.position.get())
                    .map_err(|_| ProjectStorageError::InvalidFeed)?;
                sqlx_turso::query!("INSERT INTO stand_down_acknowledgements(acknowledgement_id, assignment_id, through_epoch, through_position) VALUES (?, ?, ?, ?)", id, assignment, epoch, position).execute(&mut *self.transaction).await?;
            }
            Record::ThreadRefiled(row) => {
                let id = row.envelope.id.as_str();
                let thread = row.thread.as_str();
                sqlx_turso::query!(
                    "INSERT INTO thread_refilings(refiling_id, thread_id) VALUES (?, ?)",
                    id,
                    thread
                )
                .execute(&mut *self.transaction)
                .await?;
            }
            Record::Move(_)
            | Record::Fact(_)
            | Record::PlanRevision(_)
            | Record::TaskLinkEvent(_) => {}
            Record::Workspace(_)
            | Record::Board(_)
            | Record::Adr(_)
            | Record::ProcessDefinition(_)
            | Record::DirectoryEntry(_) => return Err(ProjectStorageError::InvalidFeed),
        }
        Ok(())
    }
}
