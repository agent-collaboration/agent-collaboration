//! Request decisions, notification resolutions and subscription facts own distinct typed rows.
use crate::ProjectStorageError;
use crate::project_actor_rows::decode_actor;
use crate::project_record_codec::{
    intent_recipient_key, stored_counter, stored_json, stored_project_row, stored_tag, stored_text,
    stored_timestamp,
};
use message_board::{
    CanonicalHash, FeedCursor, NotifyIntent, NotifyIntentId, NotifyIntentProps, ProjectId, Record,
    RecordId, RecordKind, RecordRef, RequestId, RequestLedgerEntry, SubscriptionMembership,
    SubscriptionMembershipProps, SubscriptionScope, ThreadId,
};
use sqlx::Executor;
use sqlx_turso::Turso;

stored_project_row!(StoredRequestDecision, RequestDecisionBody { canonical_hash: String, accepted_in: Option<i64>, decision: String });
stored_project_row!(
    StoredIntent,
    IntentBody {
        cause_kind: String,
        cause_id: String,
        recipient_key: String,
        recipient: String,
        state: String
    }
);
stored_project_row!(StoredMembership, MembershipBody {
    reader_actor: String, reader_identity: Option<String>, scope_kind: String, scope_id: String, policy: String,
    renewed_at: String, expires_at: String, generation: i64, activated_epoch: i64, activated_position: i64, ended: Option<String>,
});

pub(crate) enum StoredDecisionRecord {
    Request(StoredRequestDecision),
    Intent(StoredIntent),
    Membership(StoredMembership),
}
impl StoredDecisionRecord {
    pub(crate) fn reference(&self) -> Result<RecordRef, ProjectStorageError> {
        match self {
            Self::Request(row) => row.reference(RecordKind::Request),
            Self::Intent(row) => row.reference(RecordKind::NotifyIntent),
            Self::Membership(row) => row.reference(RecordKind::SubscriptionMembership),
        }
    }
    pub(crate) fn decode(self, project: &ProjectId) -> Result<Record, ProjectStorageError> {
        let reference = self.reference()?;
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        Ok(match self {
            Self::Request(row) => {
                let (raw, body) = row.into_parts();
                let envelope = raw.decode(&reference, project)?;
                let request_id = RequestId::try_from(reference.id.as_str().to_owned())
                    .map_err(|_| invalid("id"))?;
                if envelope.request.as_ref() != Some(&request_id) {
                    return Err(invalid("request"));
                }
                let decision = stored_json(&body.decision, &reference, "decision")?;
                Record::Request(RequestLedgerEntry {
                    envelope,
                    request_id,
                    canonical_hash: stored_text(
                        body.canonical_hash,
                        &reference,
                        "canonicalHash",
                        CanonicalHash::as_str,
                    )?,
                    accepted_in: body
                        .accepted_in
                        .map(|value| stored_counter(value, &reference, "acceptedIn"))
                        .transpose()?,
                    decision,
                })
            }
            Self::Intent(row) => {
                let (raw, body) = row.into_parts();
                let recipient = stored_json(&body.recipient, &reference, "recipient")?;
                if intent_recipient_key(&recipient, &reference)? != body.recipient_key {
                    return Err(invalid("recipientKey"));
                }
                Record::NotifyIntent(
                    NotifyIntent::new(NotifyIntentProps {
                        envelope: raw.decode(&reference, project)?,
                        intent_id: NotifyIntentId::try_from(reference.id.as_str().to_owned())
                            .map_err(|_| invalid("id"))?,
                        cause: RecordRef {
                            kind: stored_tag(body.cause_kind, &reference, "cause.kind")?,
                            id: stored_text(
                                body.cause_id,
                                &reference,
                                "cause.id",
                                RecordId::as_str,
                            )?,
                        },
                        recipient,
                        state: stored_json(&body.state, &reference, "state")?,
                    })
                    .map_err(|_| invalid("state"))?,
                )
            }
            Self::Membership(row) => {
                let (raw, body) = row.into_parts();
                let reader = decode_actor(
                    &body.reader_actor,
                    &body.reader_identity.ok_or_else(|| invalid("reader"))?,
                )?
                .declared_identity()
                .ok_or_else(|| invalid("reader"))?;
                let scope = match body.scope_kind.as_str() {
                    "project" if body.scope_id == project.as_str() => {
                        SubscriptionScope::project(project.clone())
                    }
                    "thread" => SubscriptionScope::thread(stored_text(
                        body.scope_id,
                        &reference,
                        "scope",
                        ThreadId::as_str,
                    )?),
                    _ => return Err(invalid("scope")),
                };
                SubscriptionMembership::new(SubscriptionMembershipProps {
                    envelope: raw.decode(&reference, project)?,
                    reader,
                    scope,
                    policy: stored_json(
                        &body.policy,
                        &reference,
                        "subscriptionMembership.policy.mode",
                    )?,
                    renewed_at: stored_timestamp(body.renewed_at, &reference, "renewedAt")?,
                    expires_at: stored_timestamp(body.expires_at, &reference, "expiresAt")?,
                    generation: stored_counter(body.generation, &reference, "generation")?,
                    activated_at: FeedCursor {
                        epoch: stored_counter(body.activated_epoch, &reference, "activatedAt")?,
                        position: stored_counter(
                            body.activated_position,
                            &reference,
                            "activatedAt",
                        )?,
                    },
                    ended: body
                        .ended
                        .map(|value| {
                            stored_json(&value, &reference, "subscriptionMembership.ended.at")
                        })
                        .transpose()?,
                })
                .map(Record::SubscriptionMembership)
                .map_err(|_| invalid("membership"))?
            }
        })
    }
}

pub(crate) async fn decision_records<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    kind: RecordKind,
    keys: &[&str],
) -> Result<Vec<StoredDecisionRecord>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let project = project.as_str();
    let keys = serde_json::to_string(keys)?;
    Ok(match kind {
        RecordKind::Request => sqlx_turso::query_file_as!(
            StoredRequestDecision,
            "src/project_decision_batch.sql",
            project,
            keys
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(StoredDecisionRecord::Request)
        .collect(),
        RecordKind::NotifyIntent => {
            sqlx_turso::query_file_as!(StoredIntent, "src/project_intent_batch.sql", project, keys)
                .fetch_all(&mut *transaction)
                .await?
                .into_iter()
                .map(StoredDecisionRecord::Intent)
                .collect()
        }
        RecordKind::SubscriptionMembership => sqlx_turso::query_file_as!(
            StoredMembership,
            "src/project_membership_batch.sql",
            project,
            keys
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(StoredDecisionRecord::Membership)
        .collect(),
        _ => return Err(ProjectStorageError::InvalidFeed),
    })
}
