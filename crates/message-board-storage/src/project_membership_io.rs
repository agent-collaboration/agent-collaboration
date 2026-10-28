//! Current subscription facts use exact reader keys; historical generations never become duplicate live scopes.
use crate::project_actor_rows::{actor_id, decode_actor};
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    Identity, MembershipEnd, ProjectId, Record, RecordId, RecordKind, RecordRef, RecordedActor,
    SubscriptionGeneration, SubscriptionMembership, SubscriptionScope, ThreadId,
};
use sqlx::Executor;
use sqlx_turso::Turso;

struct MembershipKeyRow {
    membership_id: String,
    reader_actor: String,
    reader_identity: Option<String>,
    scope_kind: String,
    scope_id: String,
    generation: i64,
    ended: Option<String>,
}
impl MembershipKeyRow {
    fn reference(&self) -> Result<RecordRef, ProjectStorageError> {
        Ok(RecordRef {
            kind: RecordKind::SubscriptionMembership,
            id: RecordId::try_from(self.membership_id.clone())
                .map_err(|_| ProjectStorageError::InvalidFeed)?,
        })
    }
    fn reader(&self) -> Result<Identity, ProjectStorageError> {
        decode_actor(
            &self.reader_actor,
            self.reader_identity
                .as_deref()
                .ok_or(ProjectStorageError::InvalidFeed)?,
        )?
        .declared_identity()
        .ok_or(ProjectStorageError::InvalidFeed)
    }
    fn scope(&self, project: &ProjectId) -> Result<SubscriptionScope, ProjectStorageError> {
        match self.scope_kind.as_str() {
            "project" if self.scope_id == project.as_str() => {
                Ok(SubscriptionScope::project(project.clone()))
            }
            "thread" => Ok(SubscriptionScope::thread(
                ThreadId::try_from(self.scope_id.clone())
                    .map_err(|_| ProjectStorageError::InvalidFeed)?,
            )),
            _ => Err(ProjectStorageError::InvalidFeed),
        }
    }
    fn validate(
        &self,
        member: &SubscriptionMembership,
        project: &ProjectId,
    ) -> Result<(), ProjectStorageError> {
        let reference = self.reference()?;
        let generation: SubscriptionGeneration =
            crate::project_record_codec::stored_counter(self.generation, &reference, "generation")?;
        if member.envelope.id != reference.id
            || member.reader() != &self.reader()?
            || member.scope() != &self.scope(project)?
            || member.generation() != generation
        {
            return Err(ProjectStorageError::InvalidRecord {
                record: reference,
                field: "membership",
            });
        }
        Ok(())
    }
}

async fn membership<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    reader: &Identity,
    scope: &SubscriptionScope,
) -> Result<Option<SubscriptionMembership>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let Some(reader_actor) =
        actor_id(transaction, &RecordedActor::declared(reader.clone())).await?
    else {
        return Ok(None);
    };
    let reader_actor = reader_actor.as_str();
    let (kind, id) = scope.kind_and_id();
    let selected = sqlx_turso::query_scalar!(r#"SELECT membership_id AS "id!: String" FROM subscription_memberships WHERE reader_actor=? AND scope_kind=? AND scope_id=? ORDER BY generation DESC LIMIT 1"#, reader_actor, kind, id).fetch_optional(&mut *transaction).await?;
    let Some(id) = selected else { return Ok(None) };
    let reference = RecordRef {
        kind: RecordKind::SubscriptionMembership,
        id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
    };
    let Some(Record::SubscriptionMembership(member)) =
        crate::project_record_io::read_record(transaction, project, &reference).await?
    else {
        return Err(ProjectStorageError::InvalidFeed);
    };
    if member.reader() != reader || member.scope() != scope {
        return Err(ProjectStorageError::InvalidRecord {
            record: reference,
            field: "membership",
        });
    }
    Ok(Some(member))
}

impl ProjectWriteTransaction {
    pub async fn unresolved_thread_memberships(
        &mut self,
        thread: &ThreadId,
    ) -> Result<Vec<SubscriptionMembership>, ProjectStorageError> {
        self.unresolved_memberships(&SubscriptionScope::thread(thread.clone()))
            .await
    }
    pub async fn unresolved_project_memberships(
        &mut self,
    ) -> Result<Vec<SubscriptionMembership>, ProjectStorageError> {
        let scope = SubscriptionScope::project(self.project_id.clone());
        self.unresolved_memberships(&scope).await
    }
    async fn unresolved_memberships(
        &mut self,
        scope: &SubscriptionScope,
    ) -> Result<Vec<SubscriptionMembership>, ProjectStorageError> {
        let (kind, id) = scope.kind_and_id();
        let rows = sqlx_turso::query_as!(MembershipKeyRow, r#"SELECT m.membership_id AS "membership_id!: String", m.reader_actor AS "reader_actor!: String", a.canonical_identity AS "reader_identity?: String", m.scope_kind AS "scope_kind!: String", m.scope_id AS "scope_id!: String", m.generation AS "generation!: i64", m.ended AS "ended?: String" FROM subscription_memberships m LEFT JOIN actors a ON a.actor_id=m.reader_actor WHERE m.scope_kind=? AND m.scope_id=? AND NOT EXISTS(SELECT 1 FROM subscription_memberships n WHERE n.reader_actor=m.reader_actor AND n.scope_kind=m.scope_kind AND n.scope_id=m.scope_id AND n.generation>m.generation) ORDER BY m.membership_id"#, kind, id).fetch_all(&mut self.transaction).await?;
        let mut expected = std::collections::BTreeMap::new();
        for row in rows {
            let reference = row.reference()?;
            let ended: Option<MembershipEnd> = row
                .ended
                .as_ref()
                .map(|value| crate::project_record_codec::stored_json(value, &reference, "ended"))
                .transpose()?;
            if ended.is_none() {
                expected.insert(reference.id, row);
            }
        }
        let keys: Vec<_> = expected
            .keys()
            .cloned()
            .map(|id| RecordRef {
                kind: RecordKind::SubscriptionMembership,
                id,
            })
            .collect();
        let bodies = self.record_batch(&keys).await?;
        if bodies.len() != keys.len() {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let mut memberships = Vec::new();
        for body in bodies {
            let key = body.reference().clone();
            let metadata = expected
                .remove(&key.id)
                .ok_or(ProjectStorageError::InvalidFeed)?;
            let Record::SubscriptionMembership(member) = body.decode()? else {
                return Err(ProjectStorageError::InvalidFeed);
            };
            metadata.validate(&member, &self.project_id)?;
            if member.ended().is_some() {
                return Err(ProjectStorageError::InvalidRecord {
                    record: key,
                    field: "ended",
                });
            }
            memberships.push(member);
        }
        Ok(memberships)
    }
    pub async fn membership(
        &mut self,
        reader: &Identity,
        scope: &SubscriptionScope,
    ) -> Result<Option<SubscriptionMembership>, ProjectStorageError> {
        membership(&mut self.transaction, &self.project_id, reader, scope).await
    }
}

impl ProjectSnapshot {
    #[tracing::instrument(skip_all, fields(project = %self.project_id.as_str()))]
    pub async fn subscription_memberships(
        &mut self,
        reader: Option<&Identity>,
    ) -> Result<Vec<Result<SubscriptionMembership, ProjectStorageError>>, ProjectStorageError> {
        let reader = reader
            .map(|reader| serde_json::to_string(&RecordedActor::declared(reader.clone())))
            .transpose()?;
        let mut after: Option<(String, String, String)> = None;
        let mut memberships = Vec::new();
        loop {
            let reader_filter = reader.as_deref();
            let after_reader = after.as_ref().map(|key| key.0.as_str());
            let after_kind = after.as_ref().map(|key| key.1.as_str());
            let after_scope = after.as_ref().map(|key| key.2.as_str());
            let mut rows = sqlx_turso::query_as!(MembershipKeyRow, r#"SELECT m.membership_id AS "membership_id!: String", m.reader_actor AS "reader_actor!: String", a.canonical_identity AS "reader_identity?: String", m.scope_kind AS "scope_kind!: String", m.scope_id AS "scope_id!: String", m.generation AS "generation!: i64", m.ended AS "ended?: String" FROM subscription_memberships m LEFT JOIN actors a ON a.actor_id=m.reader_actor WHERE (? IS NULL OR a.canonical_identity=?) AND (? IS NULL OR (a.canonical_identity,m.scope_kind,m.scope_id)>(?,?,?)) AND NOT EXISTS(SELECT 1 FROM subscription_memberships n WHERE n.reader_actor=m.reader_actor AND n.scope_kind=m.scope_kind AND n.scope_id=m.scope_id AND n.generation>m.generation) ORDER BY a.canonical_identity,m.scope_kind,m.scope_id LIMIT 101"#, reader_filter, reader_filter, after_reader, after_reader, after_kind, after_scope).fetch_all(&mut self.transaction).await?;
            let more = rows.len() > 100;
            rows.truncate(100);
            after = rows
                .last()
                .map(|row| -> Result<_, ProjectStorageError> {
                    Ok((
                        row.reader_identity
                            .clone()
                            .ok_or(ProjectStorageError::InvalidFeed)?,
                        row.scope_kind.clone(),
                        row.scope_id.clone(),
                    ))
                })
                .transpose()?;
            let keys = rows
                .iter()
                .map(MembershipKeyRow::reference)
                .collect::<Result<Vec<_>, _>>()?;
            let mut bodies: std::collections::BTreeMap<_, _> = self
                .record_batch(&keys)
                .await?
                .into_iter()
                .map(|body| (body.reference().id.clone(), body))
                .collect();
            for row in rows {
                memberships.push((|| {
                    let reference = row.reference()?;
                    let body = bodies
                        .remove(&reference.id)
                        .ok_or(ProjectStorageError::InvalidFeed)?;
                    let Record::SubscriptionMembership(member) = body.decode()? else {
                        return Err(ProjectStorageError::InvalidFeed);
                    };
                    row.validate(&member, &self.project_id)?;
                    Ok(member)
                })());
            }
            if !more {
                break;
            }
        }
        Ok(memberships)
    }
    pub async fn membership(
        &mut self,
        reader: &Identity,
        scope: &SubscriptionScope,
    ) -> Result<Option<SubscriptionMembership>, ProjectStorageError> {
        membership(&mut self.transaction, &self.project_id, reader, scope).await
    }
}
