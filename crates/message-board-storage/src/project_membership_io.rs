//! Native membership keys point to validated project records, never delivery windows.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    Identity, ProjectId, Record, RecordId, RecordKind, RecordRef, SubscriptionMembership,
    SubscriptionScope,
};

struct MembershipPageMetadata {
    reader: String,
    scope: String,
    membership_kind: String,
    membership_id: String,
    thread_id: Option<String>,
}

struct ThreadMembershipMetadata {
    reader: String,
    scope: String,
    membership_kind: String,
    membership_id: String,
    ended: String,
}
impl ProjectWriteTransaction {
    pub async fn unresolved_thread_memberships(
        &mut self,
        thread: &message_board::ThreadId,
    ) -> Result<Vec<SubscriptionMembership>, ProjectStorageError> {
        let id = thread.as_str();
        let rows = sqlx_turso::query_as!(ThreadMembershipMetadata, r#"SELECT s.reader AS "reader!: String", s.scope AS "scope!: String", s.membership_kind AS "membership_kind!: String", s.membership_id AS "membership_id!: String", json_quote(json_extract(r.payload, '$.record.ended')) AS "ended!: String" FROM thread_subscriptions s LEFT JOIN record_identities r ON r.kind = s.membership_kind AND r.id = s.membership_id WHERE s.thread_id = ? ORDER BY s.membership_id"#, id).fetch_all(&mut *self.transaction).await?;
        let mut expected = std::collections::BTreeMap::new();
        for row in rows {
            let key = RecordRef {
                kind: RecordKind::SubscriptionMembership,
                id: RecordId::try_from(row.membership_id)
                    .map_err(|_| ProjectStorageError::InvalidFeed)?,
            };
            let invalid = || ProjectStorageError::InvalidRecord {
                record: key.clone(),
                field: "membership",
            };
            if row.membership_kind != "subscriptionMembership" {
                return Err(invalid());
            }
            let reader: Identity = serde_json::from_str(&row.reader).map_err(|_| invalid())?;
            let scope: SubscriptionScope =
                serde_json::from_str(&row.scope).map_err(|_| invalid())?;
            // These are canonical serialized primary keys; body identities below are semantic.
            if serde_json::to_string(&reader)? != row.reader
                || serde_json::to_string(&scope)? != row.scope
                || scope.thread_id() != Some(thread)
            {
                return Err(invalid());
            }
            let ended: Option<message_board::MembershipEnd> =
                serde_json::from_str(&row.ended).map_err(|_| invalid())?;
            if ended.is_some() {
                continue;
            }
            if expected.insert(key.id.clone(), (reader, scope)).is_some() {
                return Err(invalid());
            }
        }
        if expected.is_empty() {
            return Ok(Vec::new());
        }
        let keys: Vec<_> = expected
            .keys()
            .cloned()
            .map(|id| RecordRef {
                kind: RecordKind::SubscriptionMembership,
                id,
            })
            .collect();
        let mut memberships = Vec::new();
        for row in self.record_batch(&keys).await? {
            let key = row.reference().clone();
            let invalid = || ProjectStorageError::InvalidRecord {
                record: key.clone(),
                field: "membership",
            };
            let Some((reader, scope)) = expected.remove(&key.id) else {
                return Err(invalid());
            };
            let Record::SubscriptionMembership(member) = row.decode()? else {
                return Err(invalid());
            };
            if member.reader() != &reader || member.scope() != &scope || member.ended().is_some() {
                return Err(invalid());
            }
            memberships.push(member);
        }
        if let Some(id) = expected.into_keys().next() {
            return Err(ProjectStorageError::InvalidRecord {
                record: RecordRef {
                    kind: RecordKind::SubscriptionMembership,
                    id,
                },
                field: "membership",
            });
        }
        Ok(memberships)
    }
}

async fn membership(
    connection: &mut sqlx_turso::TursoConnection,
    project: &ProjectId,
    reader: &Identity,
    scope: &SubscriptionScope,
) -> Result<Option<SubscriptionMembership>, ProjectStorageError> {
    let reader_key = serde_json::to_string(reader)?;
    let scope_key = serde_json::to_string(scope)?;
    let row = sqlx_turso::query!(r#"SELECT membership_kind AS "membership_kind!: String", membership_id AS "membership_id!: String", thread_id AS "thread_id: String" FROM thread_subscriptions WHERE reader = ? AND scope = ?"#, reader_key, scope_key)
        .fetch_optional(&mut *connection).await?;
    let Some(row) = row else { return Ok(None) };
    let reference = RecordRef {
        kind: RecordKind::SubscriptionMembership,
        id: RecordId::try_from(row.membership_id).map_err(|_| ProjectStorageError::InvalidFeed)?,
    };
    let invalid = || ProjectStorageError::InvalidRecord {
        record: reference.clone(),
        field: "membership",
    };
    if row.membership_kind != "subscriptionMembership" {
        return Err(invalid());
    }
    let Some(record) = crate::project_record_io::read_record(connection, &reference).await? else {
        return Err(invalid());
    };
    crate::project_record_io::validate_project_record(&record, project)?;
    let Record::SubscriptionMembership(membership) = record else {
        return Err(invalid());
    };
    if membership.reader() != reader
        || membership.scope() != scope
        || row.thread_id.as_deref() != scope.thread_id().map(|thread| thread.as_str())
    {
        return Err(invalid());
    }
    Ok(Some(membership))
}

impl ProjectSnapshot {
    /// Read only the selected reader's memberships, in native primary-key pages.
    #[tracing::instrument(skip_all, fields(project = %self.project_id.as_str()))]
    pub async fn subscription_memberships(
        &mut self,
        reader: Option<&Identity>,
    ) -> Result<Vec<Result<SubscriptionMembership, ProjectStorageError>>, ProjectStorageError> {
        let reader_key = reader.map(serde_json::to_string).transpose()?;
        let mut after: Option<(String, String)> = None;
        let mut memberships = Vec::new();
        loop {
            let mut rows = match (&reader_key, &after) {
                (Some(reader), None) => sqlx_turso::query_as!(MembershipPageMetadata, r#"SELECT reader AS "reader!: String", scope AS "scope!: String", membership_kind AS "membership_kind!: String", membership_id AS "membership_id!: String", thread_id AS "thread_id: String" FROM thread_subscriptions WHERE reader = ? ORDER BY scope LIMIT 101"#, reader).fetch_all(&mut *self.transaction).await?,
                (Some(reader), Some((_, scope))) => sqlx_turso::query_as!(MembershipPageMetadata, r#"SELECT reader AS "reader!: String", scope AS "scope!: String", membership_kind AS "membership_kind!: String", membership_id AS "membership_id!: String", thread_id AS "thread_id: String" FROM thread_subscriptions WHERE reader = ? AND scope > ? ORDER BY scope LIMIT 101"#, reader, scope).fetch_all(&mut *self.transaction).await?,
                (None, None) => sqlx_turso::query_as!(MembershipPageMetadata, r#"SELECT reader AS "reader!: String", scope AS "scope!: String", membership_kind AS "membership_kind!: String", membership_id AS "membership_id!: String", thread_id AS "thread_id: String" FROM thread_subscriptions ORDER BY reader, scope LIMIT 101"#).fetch_all(&mut *self.transaction).await?,
                (None, Some((reader, scope))) => sqlx_turso::query_file_as!(MembershipPageMetadata, "src/project_membership_page.sql", reader, scope, reader).fetch_all(&mut *self.transaction).await?,
            };
            let has_more = rows.len() > 100;
            rows.truncate(100);
            after = rows
                .last()
                .map(|row| (row.reader.clone(), row.scope.clone()));
            let mut keys = Vec::new();
            for row in &rows {
                if row.membership_kind == "subscriptionMembership" {
                    keys.push(RecordRef {
                        kind: RecordKind::SubscriptionMembership,
                        id: RecordId::try_from(row.membership_id.clone())
                            .map_err(|_| ProjectStorageError::InvalidFeed)?,
                    });
                }
            }
            let mut stored: std::collections::BTreeMap<_, std::collections::VecDeque<_>> =
                std::collections::BTreeMap::new();
            if !keys.is_empty() {
                for record in self.record_batch(&keys).await? {
                    stored
                        .entry(record.reference().id.clone())
                        .or_default()
                        .push_back(record);
                }
            }
            for row in rows {
                let key = RecordRef {
                    kind: RecordKind::SubscriptionMembership,
                    id: RecordId::try_from(row.membership_id)
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                };
                let decoded = (|| {
                    let invalid = |field| ProjectStorageError::InvalidRecord {
                        record: key.clone(),
                        field,
                    };
                    let reader: Identity =
                        serde_json::from_str(&row.reader).map_err(|_| invalid("membershipKey"))?;
                    let scope: SubscriptionScope =
                        serde_json::from_str(&row.scope).map_err(|_| invalid("membershipKey"))?;
                    if row.membership_kind != "subscriptionMembership"
                        || serde_json::to_string(&reader)? != row.reader
                        || serde_json::to_string(&scope)? != row.scope
                    {
                        return Err(invalid("membership"));
                    }
                    let body = stored
                        .get_mut(&key.id)
                        .and_then(std::collections::VecDeque::pop_front)
                        .ok_or_else(|| invalid("membership"))?;
                    let Record::SubscriptionMembership(member) = body.decode()? else {
                        return Err(invalid("membership"));
                    };
                    if member.reader() != &reader
                        || member.scope() != &scope
                        || row.thread_id.as_deref()
                            != scope.thread_id().map(|thread| thread.as_str())
                    {
                        return Err(invalid("membership"));
                    }
                    Ok(member)
                })();
                memberships.push(decoded);
            }
            if !has_more {
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
impl ProjectWriteTransaction {
    pub async fn membership(
        &mut self,
        reader: &Identity,
        scope: &SubscriptionScope,
    ) -> Result<Option<SubscriptionMembership>, ProjectStorageError> {
        membership(&mut self.transaction, &self.project_id, reader, scope).await
    }
}
