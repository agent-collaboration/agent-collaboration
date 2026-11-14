use crate::workspace_decode_error::corrupt;
use crate::workspace_row_codec::{json, string};
use crate::{ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch, StoreFailure,
};
use workspace_model::{
    ActorIdentity, ActorIdentityId, ActorRef, ActorView, Identity, RefusalReason,
};

fn text(value: impl ToString) -> SqlValue {
    SqlValue::Text(value.to_string())
}

fn identity_fields(identity: &Identity) -> Result<(&'static str, String), StoreFailure> {
    let kind = match identity {
        Identity::AgentSession { .. } => "agentSession",
        Identity::RouterComponent { .. } => "routerComponent",
        Identity::Person { .. } => "person",
    };
    let encoded = serde_json_canonicalizer::to_string(identity).map_err(|_| corrupt("identity"))?;
    Ok((kind, encoded))
}

fn identity_lookup(kind: &str, encoded: &str) -> Statement {
    Statement {
        sql: "SELECT actor_identity_id FROM actor_identities WHERE identity_kind=? AND identity=?",
        args: vec![text(kind), text(encoded)],
    }
}

fn identity_insert(actor: &ActorIdentityId, kind: &str, encoded: &str, now: &str) -> Statement {
    Statement {
        sql: "INSERT INTO actor_identities (actor_identity_id,identity_kind,identity,created_at,updated_at) VALUES (?,?,?,?,?) ON CONFLICT(identity_kind,identity) DO NOTHING",
        args: vec![text(actor), text(kind), text(encoded), text(now), text(now)],
    }
}

/// A new caller identity joins the same atomic batch as its retained outcome.
pub(crate) struct ActorIdentityPlan {
    pub actor: ActorIdentityId,
    pub statements: Vec<GuardedStatement>,
}

impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    /// Caller holds the org turn and supplies its sole applying time sample.
    pub(crate) async fn prepare_identity(
        &self,
        identity: &Identity,
        now: workspace_model::Timestamp,
    ) -> Result<ActorIdentityPlan, StoreFailure> {
        let (kind, encoded) = identity_fields(identity)?;
        let rows = self.host.read(identity_lookup(kind, &encoded)).await?;
        if rows.rows.len() > 1 {
            return Err(corrupt("actor_identities.identity"));
        }
        if let Some(row) = rows.rows.first() {
            let actor = string(&rows, row, "actor_identity_id")?
                .parse()
                .map_err(|_| corrupt("actor_identities.actor_identity_id"))?;
            return Ok(ActorIdentityPlan {
                actor,
                statements: vec![],
            });
        }
        let actor: ActorIdentityId = self.mint()?;
        let statement = identity_insert(&actor, kind, &encoded, &now.to_rfc3339());
        Ok(ActorIdentityPlan {
            actor,
            statements: vec![GuardedStatement {
                statement,
                expect: RowExpectation::Exactly(1),
            }],
        })
    }

    pub(crate) async fn resolve_identity(
        &self,
        identity: &Identity,
    ) -> Result<ActorIdentityId, StoreFailure> {
        let (kind, encoded) = identity_fields(identity)?;
        let lookup = || identity_lookup(kind, &encoded);
        let existing = self.host.read(lookup()).await?;
        if let Some(row) = existing.rows.first() {
            return string(&existing, row, "actor_identity_id")?
                .parse()
                .map_err(|_| corrupt("actor_identity_id"));
        }
        let minted: ActorIdentityId = self.mint()?;
        let now = self.now()?.to_rfc3339();
        let batch = StatementBatch {
            statements: vec![GuardedStatement {
                statement: identity_insert(&minted, kind, &encoded, &now),
                expect: RowExpectation::Any,
            }],
            alarm: AlarmUpdate::Unchanged,
        };
        if !matches!(
            self.host.run_batch(batch).await?,
            BatchOutcome::Applied { .. }
        ) {
            return Err(corrupt("actor insertion"));
        }
        let stored = self.host.read(lookup()).await?;
        let row = stored
            .rows
            .first()
            .ok_or_else(|| corrupt("actor_identity_id"))?;
        string(&stored, row, "actor_identity_id")?
            .parse()
            .map_err(|_| corrupt("actor_identity_id"))
    }
    pub(crate) async fn resolve_actor_ref(
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
                let _turn = self.take_central_turn().await;
                self.resolve_identity(&Identity::AgentSession {
                    session: session.clone(),
                })
                .await
                .map(Ok)
            }
        }
    }
    pub(crate) async fn actor_views(
        &self,
        ids: &[ActorIdentityId],
    ) -> Result<Vec<ActorView>, StoreFailure> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let encoded = serde_json::to_string(ids).map_err(|_| corrupt("actor ids"))?;
        let rows=self.host.read(Statement {sql:"SELECT actor_identity_id,identity_kind,identity FROM actor_identities WHERE actor_identity_id IN (SELECT value FROM json_each(?))",args:vec![text(encoded)]}).await?;
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
    let identity: Identity = json(string(rows, row, "identity")?, "actor_identities.identity")?;
    let expected = match identity {
        Identity::AgentSession { .. } => "agentSession",
        Identity::RouterComponent { .. } => "routerComponent",
        Identity::Person { .. } => "person",
    };
    if string(rows, row, "identity_kind")? != expected {
        return Err(corrupt("actor_identities.identity_kind"));
    }
    Ok(ActorIdentity {
        actor_identity_id: string(rows, row, "actor_identity_id")?
            .parse()
            .map_err(|_| corrupt("actor_identity_id"))?,
        identity,
    })
}
impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn named_actor(&self, actor: &ActorIdentityId) -> Result<bool, StoreFailure> {
        let rows=self.host.read(Statement{sql:"SELECT actor_identity_id,identity_kind,identity FROM actor_identities WHERE actor_identity_id=?",args:vec![text(actor)]}).await?;
        match rows.rows.first() {
            Some(row) => {
                decode_actor(&rows, row)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }
}
