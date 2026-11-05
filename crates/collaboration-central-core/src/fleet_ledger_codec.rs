//! Typed immutable ledger headers keep their original scope, placement and decision commitment.
use crate::{
    fleet_column_codec::{ActorColumns, canonical, decimal, optional_text, text, unsigned},
    fleet_record_codec::FleetRecordHeader,
    fleet_row_codec::{check_millis, corrupt},
    generated_statements::StoredFleetDecision,
};
use durable_steps::StoreError;
use message_board::{
    CanonicalHash, ChangeOutcome, DecisionEffectsHeader, FleetFeedEntry, FleetPosition,
    RecordEnvelope, RecordPlacement, RecordScope, RequestId, RequestLedgerEntry, StoredDecision,
};
pub(crate) enum DecisionHeader {
    Accepted {
        change: ChangeOutcome,
        effects: DecisionEffectsHeader,
    },
    Rejected {
        roots: u64,
        nodes: u64,
    },
}
pub(crate) struct LedgerHeader {
    pub request: RequestId,
    pub hash: CanonicalHash,
    pub envelope: RecordEnvelope,
    pub decision: DecisionHeader,
}
pub(crate) fn decode(row: StoredFleetDecision) -> Result<LedgerHeader, StoreError> {
    let request: RequestId = canonical(
        text(row.request_id, row.request_id_class, "ledger.request")?,
        "ledger.request",
    )?;
    let hash = canonical(
        text(row.canonical_hash, row.canonical_hash_class, "ledger.hash")?,
        "ledger.hash",
    )?;
    check_millis(row.created_at, row.created_at_class, "ledger.created_at")?;
    check_millis(row.updated_at, row.updated_at_class, "ledger.updated_at")?;
    let header = FleetRecordHeader::decode(
        optional_text(row.router_time, row.router_time_class, "ledger.router_time")?,
        Some(text(
            row.time_basis,
            row.time_basis_class,
            "ledger.time_basis",
        )?),
        Some(text(
            row.origin_installation,
            row.origin_installation_class,
            "ledger.origin_installation",
        )?),
        Some(text(
            row.origin_machine,
            row.origin_machine_class,
            "ledger.origin_machine",
        )?),
    )?;
    if header.time.as_router_time().is_none() {
        return Err(corrupt(
            "ledger.time_basis",
            "a live request decision needs router time",
        ));
    }
    let actor = ActorColumns {
        kind: text(row.actor_kind, row.actor_kind_class, "ledger.actor_kind")?,
        human: optional_text(
            row.actor_human_id,
            row.actor_human_id_class,
            "ledger.actor_human_id",
        )?,
        service: optional_text(
            row.actor_service_id,
            row.actor_service_id_class,
            "ledger.actor_service_id",
        )?,
        endpoint: optional_text(
            row.actor_endpoint_id,
            row.actor_endpoint_id_class,
            "ledger.actor_endpoint_id",
        )?,
        session: optional_text(
            row.actor_session_id,
            row.actor_session_id_class,
            "ledger.actor_session_id",
        )?,
        component: optional_text(
            row.actor_router_component,
            row.actor_router_component_class,
            "ledger.actor_router_component",
        )?,
    }
    .decode()?;
    let position = FleetPosition::try_from(unsigned(
        row.fleet_position,
        row.fleet_position_class,
        "ledger.position",
    )?)
    .map_err(|error| corrupt("ledger.position", error))?;
    let envelope = RecordEnvelope {
        id: request.clone().into(),
        scope: RecordScope {
            workspace: canonical(
                text(row.workspace_id, row.workspace_id_class, "ledger.workspace")?,
                "ledger.workspace",
            )?,
            board: optional_text(row.board_id, row.board_id_class, "ledger.board")?
                .map(|value| canonical(value, "ledger.board"))
                .transpose()?,
            project: None,
        },
        actor,
        request: optional_text(
            row.envelope_request_id,
            row.envelope_request_id_class,
            "ledger.envelope_request",
        )?
        .map(|value| canonical(value, "ledger.envelope_request"))
        .transpose()?,
        position: RecordPlacement::Fleet(FleetFeedEntry {
            position,
            origin: header.origin,
        }),
        time: header.time,
    };
    let roots = unsigned(
        row.reason_count,
        row.reason_count_class,
        "ledger.reason_count",
    )?;
    let nodes = unsigned(
        row.stored_reason_nodes,
        row.stored_reason_nodes_class,
        "ledger.reason_nodes",
    )?;
    let change = optional_text(row.change_kind, row.change_kind_class, "ledger.change")?;
    let count = optional_text(
        row.effects_count,
        row.effects_count_class,
        "ledger.effects_count",
    )?;
    let digest = optional_text(
        row.effects_digest,
        row.effects_digest_class,
        "ledger.effects_digest",
    )?;
    let decision = match (
        text(
            row.decision_kind,
            row.decision_kind_class,
            "ledger.decision",
        )?
        .as_str(),
        change,
        count,
        digest,
        roots,
        nodes,
    ) {
        ("accepted", Some(change), Some(count), Some(digest), 0, 0) => DecisionHeader::Accepted {
            change: match change.as_str() {
                "changed" => ChangeOutcome::Changed,
                "already_so" => ChangeOutcome::AlreadySo,
                _ => return Err(corrupt("ledger.change", "unknown change outcome")),
            },
            effects: DecisionEffectsHeader {
                count: decimal(&count, "ledger.effects_count")?,
                digest: canonical(digest, "ledger.effects_digest")?,
            },
        },
        ("rejected", None, None, None, 1, nodes) if (1..=2).contains(&nodes) => {
            DecisionHeader::Rejected { roots: 1, nodes }
        }
        _ => {
            return Err(corrupt(
                "ledger.decision",
                "unknown decision, inactive fields or inconsistent reason nodes",
            ));
        }
    };
    Ok(LedgerHeader {
        request,
        hash,
        envelope,
        decision,
    })
}
impl LedgerHeader {
    pub(crate) fn rejected(&self) -> bool {
        matches!(self.decision, DecisionHeader::Rejected { .. })
    }
    pub(crate) fn record(
        self,
        trees: &mut crate::fleet_reason_tree::ReasonTrees,
    ) -> Result<RequestLedgerEntry, StoreError> {
        let decision = match self.decision {
            DecisionHeader::Accepted { change, effects } => {
                StoredDecision::Accepted { change, effects }
            }
            DecisionHeader::Rejected { roots, nodes } => StoredDecision::Rejected {
                reasons: trees.take(&self.request, roots, nodes)?,
            },
        };
        let entry = RequestLedgerEntry {
            request_id: self.request,
            canonical_hash: self.hash,
            envelope: self.envelope,
            accepted_in: None,
            decision,
        };
        crate::fleet_record_codec::validate_ledger(&entry)?;
        Ok(entry)
    }
}

pub(crate) async fn insert<S: durable_steps::storage::TransactionScope>(
    scope: &mut S,
    entry: &RequestLedgerEntry,
    now: runtime_primitives::SyncTime,
) -> Result<(), StoreError>
where
    <S::Database as sqlx_core::database::Database>::Arguments:
        sqlx_core::arguments::IntoArguments<S::Database>,
    for<'v> String:
        sqlx_core::types::Type<S::Database> + sqlx_core::encode::Encode<'v, S::Database>,
    for<'v> Option<String>:
        sqlx_core::types::Type<S::Database> + sqlx_core::encode::Encode<'v, S::Database>,
{
    crate::fleet_record_codec::validate_ledger(entry)?;
    let time = entry
        .envelope
        .time
        .as_router_time()
        .ok_or_else(|| corrupt("ledger.time_basis", "live decisions require Router time"))?;
    let origin = entry
        .envelope
        .position
        .origin()
        .ok_or_else(|| corrupt("ledger.origin", "decision has no Fleet origin"))?;
    let position = entry
        .envelope
        .position
        .fleet_position()
        .ok_or_else(|| corrupt("ledger.position", "decision is not in Fleet"))?;
    i64::try_from(position.get()).map_err(|error| corrupt("ledger.position", error))?;
    let actor = ActorColumns::encode(&entry.envelope.actor);
    let (kind, change, count, digest, reasons) = match &entry.decision {
        StoredDecision::Accepted { change, effects } => (
            "accepted",
            Some(
                match change {
                    ChangeOutcome::Changed => "changed",
                    ChangeOutcome::AlreadySo => "already_so",
                }
                .into(),
            ),
            Some(effects.count.to_string()),
            Some(effects.digest.as_str().into()),
            0_usize,
        ),
        StoredDecision::Rejected { reasons } => ("rejected", None, None, None, reasons.len()),
    };
    crate::generated_statements::insert_fleet_decision(
        scope,
        entry.request_id.as_str().into(),
        entry.canonical_hash.as_str().into(),
        entry.envelope.scope.workspace.as_str().into(),
        entry
            .envelope
            .scope
            .board
            .as_ref()
            .map(|id| id.as_str().into()),
        actor.kind,
        actor.human,
        actor.service,
        actor.endpoint,
        actor.session,
        actor.component,
        entry.envelope.request.as_ref().map(|id| id.as_str().into()),
        position.get().to_string(),
        Some(time.to_rfc3339()),
        "router".into(),
        origin.installation.as_str().into(),
        origin.machine.as_str().into(),
        now.unix_millis().to_string(),
        kind.into(),
        change,
        count,
        digest,
        reasons.to_string(),
    )
    .await
    .map_err(StoreError::backend)
}
