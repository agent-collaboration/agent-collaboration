use crate::WorkspaceAccess;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch, StoreFailure,
};
use fleet_runtime_ports::stored_values::{json, string};
use fleet_runtime_ports::workspace_decode_error::corrupt;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_model::{
    ActorIdentity, ActorIdentityId, ActorRef, ActorView, Identity, RefusalReason, Timestamp,
    central_mail::NoticeCadence,
};

fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}

fn identity_fields(identity: &Identity) -> Result<(&'static str, String), StoreFailure> {
    let kind = identity.stored_kind();
    let encoded = serde_json_canonicalizer::to_string(identity).map_err(|_| corrupt("identity"))?;
    Ok((kind, encoded))
}

fn identity_lookup(kind: &str, encoded: &str) -> Statement {
    Statement {
        sql: "SELECT actor_identity_id FROM access_actor_identities WHERE identity_kind=? AND identity=?",
        args: vec![text(kind), text(encoded)],
    }
}

fn identity_insert(actor: &ActorIdentityId, kind: &str, encoded: &str, now: &str) -> Statement {
    Statement {
        sql: "INSERT INTO access_actor_identities (actor_identity_id,identity_kind,identity,created_at,updated_at) VALUES (?,?,?,?,?) ON CONFLICT(identity_kind,identity) DO NOTHING",
        args: vec![text(actor), text(kind), text(encoded), text(now), text(now)],
    }
}

/// A new caller identity joins the same atomic batch as its retained outcome.
pub struct ActorIdentityPlan {
    pub actor: ActorIdentityId,
    pub statements: Vec<GuardedStatement>,
}

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceAccess<'_, Host, Stores> {
    /// Find the caller's actor without minting one on a status read.
    pub async fn existing_caller_actor(
        &self,
        caller: &workspace_model::Caller,
    ) -> Result<Option<ActorIdentityId>, StoreFailure> {
        let identity = match caller {
            workspace_model::Caller::Session(session) => Identity::AgentSession {
                session: session.clone(),
            },
            workspace_model::Caller::Person { person_id, .. } => Identity::Person {
                person_id: person_id.clone(),
            },
        };
        let (kind, encoded) = identity_fields(&identity)?;
        let rows = self
            .execution
            .statement_host()
            .read(identity_lookup(kind, &encoded))
            .await?;
        match rows.rows.as_slice() {
            [] => Ok(None),
            [row] => string(&rows, row, "actor_identity_id")?
                .parse()
                .map(Some)
                .map_err(|_| corrupt("access_actor_identities.actor_identity_id")),
            _ => Err(corrupt("access_actor_identities.identity")),
        }
    }

    /// Caller holds the org turn and supplies its sole applying time sample.
    pub async fn prepare_identity(
        &self,
        identity: &Identity,
        now: workspace_model::Timestamp,
    ) -> Result<ActorIdentityPlan, StoreFailure> {
        let (kind, encoded) = identity_fields(identity)?;
        let rows = self
            .execution
            .statement_host()
            .read(identity_lookup(kind, &encoded))
            .await?;
        if rows.rows.len() > 1 {
            return Err(corrupt("access_actor_identities.identity"));
        }
        if let Some(row) = rows.rows.first() {
            let actor = string(&rows, row, "actor_identity_id")?
                .parse()
                .map_err(|_| corrupt("access_actor_identities.actor_identity_id"))?;
            return Ok(ActorIdentityPlan {
                actor,
                statements: vec![],
            });
        }
        let actor: ActorIdentityId = self.execution.mint()?;
        let statement = identity_insert(&actor, kind, &encoded, &now.to_rfc3339());
        Ok(ActorIdentityPlan {
            actor,
            statements: vec![GuardedStatement {
                statement,
                expect: RowExpectation::Exactly(1),
            }],
        })
    }

    pub async fn resolve_identity(
        &self,
        identity: &Identity,
    ) -> Result<ActorIdentityId, StoreFailure> {
        let (kind, encoded) = identity_fields(identity)?;
        let lookup = || identity_lookup(kind, &encoded);
        let existing = self.execution.statement_host().read(lookup()).await?;
        if let Some(row) = existing.rows.first() {
            return string(&existing, row, "actor_identity_id")?
                .parse()
                .map_err(|_| corrupt("actor_identity_id"));
        }
        let minted: ActorIdentityId = self.execution.mint()?;
        let now = self.execution.now()?.to_rfc3339();
        let batch = StatementBatch {
            statements: vec![GuardedStatement {
                statement: identity_insert(&minted, kind, &encoded, &now),
                expect: RowExpectation::Any,
            }],
            alarm: AlarmUpdate::Unchanged,
        };
        match self.execution.statement_host().run_batch(batch).await? {
            BatchOutcome::Applied { .. } => {}
            // A rejected statement keeps main's store-outage answer on this path.
            BatchOutcome::StatementRejected { .. } => return Err(StoreFailure::Unavailable),
            BatchOutcome::GuardFailed { .. } | BatchOutcome::ConstraintViolated { .. } => {
                return Err(corrupt("actor insertion"));
            }
        }
        let stored = self.execution.statement_host().read(lookup()).await?;
        let row = stored
            .rows
            .first()
            .ok_or_else(|| corrupt("actor_identity_id"))?;
        string(&stored, row, "actor_identity_id")?
            .parse()
            .map_err(|_| corrupt("actor_identity_id"))
    }
    pub async fn resolve_actor_ref(
        &self,
        reference: &ActorRef,
        caller: &ActorIdentityId,
    ) -> Result<Result<ActorIdentityId, RefusalReason>, StoreFailure> {
        match reference {
            ActorRef::Me => Ok(Ok(caller.clone())),
            ActorRef::Actor(actor) => Ok(if self.named_actor(actor).await? {
                Ok(actor.clone())
            } else {
                Err(RefusalReason::UnknownActor {
                    actor_identity_id: actor.clone(),
                })
            }),
            ActorRef::Session(session) => {
                let _turn = self.execution.take_central_turn().await;
                self.resolve_identity(&Identity::AgentSession {
                    session: session.clone(),
                })
                .await
                .map(Ok)
            }
        }
    }
    pub async fn actor_views(
        &self,
        ids: &[ActorIdentityId],
    ) -> Result<Vec<ActorView>, StoreFailure> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let encoded = serde_json::to_string(ids).map_err(|_| corrupt("actor ids"))?;
        let rows=self.execution.statement_host().read(Statement {sql:"SELECT actor_identity_id,identity_kind,identity FROM access_actor_identities WHERE actor_identity_id IN (SELECT value FROM json_each(?))",args:vec![text(encoded)]}).await?;
        let mut actors = Vec::new();
        for row in &rows.rows {
            actors.push(ActorView::from(decode_actor(&rows, row)?));
        }
        if ids
            .iter()
            .any(|id| !actors.iter().any(|actor| actor.actor_identity_id == *id))
        {
            return Err(corrupt("missing actor identity"));
        }
        Ok(actors)
    }
}

fn decode_actor(
    rows: &central_storage::ReadOutcome,
    row: &[SqlValue],
) -> Result<ActorIdentity, StoreFailure> {
    let identity: Identity = json(
        string(rows, row, "identity")?,
        "access_actor_identities.identity",
    )?;
    let expected = match identity {
        Identity::AgentSession { .. } => "agentSession",
        Identity::RouterComponent { .. } => "routerComponent",
        Identity::Person { .. } => "person",
    };
    if string(rows, row, "identity_kind")? != expected {
        return Err(corrupt("access_actor_identities.identity_kind"));
    }
    Ok(ActorIdentity {
        actor_identity_id: string(rows, row, "actor_identity_id")?
            .parse()
            .map_err(|_| corrupt("actor_identity_id"))?,
        identity,
    })
}
impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceAccess<'_, Host, Stores> {
    pub async fn named_actor(&self, actor: &ActorIdentityId) -> Result<bool, StoreFailure> {
        Ok(self.read_actor_identity(actor).await?.is_some())
    }

    pub async fn read_actor_identity(
        &self,
        actor: &ActorIdentityId,
    ) -> Result<Option<Identity>, StoreFailure> {
        let rows=self.execution.statement_host().read(Statement{sql:"SELECT actor_identity_id,identity_kind,identity FROM access_actor_identities WHERE actor_identity_id=?",args:vec![text(actor)]}).await?;
        match rows.rows.as_slice() {
            [] => Ok(None),
            [row] => Ok(Some(decode_actor(&rows, row)?.identity)),
            _ => Err(corrupt("access_actor_identities.actor_identity_id")),
        }
    }
}

/// Plans the guarded write of an admitted session's notice cadence.
pub fn plan_notice_cadence(
    actor: &ActorIdentityId,
    cadence: NoticeCadence,
    updated_at: Timestamp,
) -> Result<GuardedStatement, StoreFailure> {
    use fleet_runtime_ports::guarded_statements::{exact, text};
    let encoded = serde_json::to_value(cadence).map_err(|_| corrupt("notice cadence"))?;
    let cadence_text = encoded.as_str().ok_or_else(|| corrupt("notice cadence"))?;
    let updated_at = updated_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    Ok(exact(
        "UPDATE access_actor_identities SET notice_cadence=?,updated_at=? WHERE actor_identity_id=?",
        vec![text(cadence_text), text(updated_at), text(actor)],
    ))
}
