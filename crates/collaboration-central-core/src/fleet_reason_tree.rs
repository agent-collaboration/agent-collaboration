//! A request owns one ordered refusal root and at most one project-rejection child.
use crate::{
    fleet_reason_evidence::{EvidenceEntry, EvidenceRow},
    fleet_reason_fields::{ReasonFields, ReasonNode},
    fleet_row_codec::corrupt,
    generated_fleet_reasons as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::{Refusal, RequestId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct EncodedReason {
    pub id: String,
    pub parent: Option<String>,
    pub ordinal: usize,
    pub fields: ReasonFields,
    pub evidence: Vec<EvidenceEntry>,
}
pub(crate) fn encode(
    reasons: &[Refusal],
    ids: &mut crate::fleet_row_ids::FleetRowIds,
) -> Result<Vec<EncodedReason>, StoreError> {
    if reasons.len() != 1 {
        return Err(corrupt(
            "reason.roots",
            "a rejected ledger requires exactly one root",
        ));
    }
    let mut nodes = Vec::new();
    for (ordinal, reason) in reasons.iter().enumerate() {
        reason
            .validate_bounds()
            .map_err(|error| corrupt("reason", error))?;
        let id = ids.next()?.to_string();
        let (fields, evidence) = crate::fleet_refusal_codec::encode(reason)?;
        nodes.push(EncodedReason {
            id: id.clone(),
            parent: None,
            ordinal,
            fields,
            evidence,
        });
        if let Refusal::ProjectRejected { reason } = reason {
            let (fields, evidence) = crate::fleet_refusal_codec::encode(reason)?;
            nodes.push(EncodedReason {
                id: ids.next()?.to_string(),
                parent: Some(id),
                ordinal: 0,
                fields,
                evidence,
            });
        }
    }
    Ok(nodes)
}

#[derive(Default)]
pub(crate) struct ReasonTrees {
    nodes: BTreeMap<RequestId, Vec<ReasonNode>>,
    evidence: BTreeMap<String, Vec<EvidenceRow>>,
}
impl ReasonTrees {
    pub(crate) async fn read<S: TransactionScope>(
        scope: &mut S,
        requests: &[RequestId],
    ) -> Result<Self, StoreError>
    where
        <S::Database as Database>::Arguments: IntoArguments<S::Database>,
        for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
        for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
        for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
    {
        let mut trees = Self::default();
        if requests.is_empty() {
            return Ok(trees);
        }
        let expected: BTreeSet<_> = requests.iter().cloned().collect();
        let ids =
            serde_json::to_string(&expected.iter().map(RequestId::as_str).collect::<Vec<_>>())
                .map_err(StoreError::backend)?;
        // Shared Record bounds allow one root and one optional ProjectRejected child.
        let node_limit = expected
            .len()
            .checked_mul(2)
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| corrupt("reason.count", "batch count overflow"))?;
        let rows = queries::read_decision_reasons(scope, ids.clone(), node_limit.to_string())
            .await
            .map_err(StoreError::backend)?;
        if rows.len() == node_limit {
            return Err(corrupt(
                "reason.count",
                "more nodes than the shared refusal bound",
            ));
        }
        for row in rows {
            let node = crate::fleet_reason_fields::decode_node(row)?;
            if !expected.contains(&node.request) {
                return Err(corrupt("reason.request", "unrequested reason tree"));
            }
            trees
                .nodes
                .entry(node.request.clone())
                .or_default()
                .push(node);
        }
        let evidence_limit = expected
            .len()
            .checked_mul(message_board::MAX_EVIDENCE_ENTRIES)
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| corrupt("reason.evidence.count", "batch count overflow"))?;
        let rows = queries::read_decision_evidence(scope, ids, evidence_limit.to_string())
            .await
            .map_err(StoreError::backend)?;
        if rows.len() == evidence_limit {
            return Err(corrupt(
                "reason.evidence.count",
                "more evidence than the shared refusal bound",
            ));
        }
        for row in rows {
            let row = crate::fleet_reason_evidence::decode(row)?;
            trees
                .evidence
                .entry(row.reason.clone())
                .or_default()
                .push(row);
        }
        Ok(trees)
    }

    pub(crate) fn take(
        &mut self,
        request: &RequestId,
        count: u64,
        expected_nodes: u64,
    ) -> Result<Vec<Refusal>, StoreError> {
        let nodes = self.nodes.remove(request).unwrap_or_default();
        if count != 1
            || nodes.is_empty()
            || nodes.len() > 2
            || usize::try_from(expected_nodes).ok() != Some(nodes.len())
        {
            return Err(corrupt(
                "reason.roots",
                "root count or bounded node set differs",
            ));
        }
        let mut by_id = BTreeMap::new();
        for node in nodes {
            if node.request != *request || by_id.insert(node.id.clone(), node).is_some() {
                return Err(corrupt(
                    "reason.request",
                    "duplicate or cross-request reason",
                ));
            }
        }
        for node in by_id.values() {
            if let Some(parent) = &node.parent
                && !by_id.contains_key(parent)
            {
                return Err(corrupt("reason.parent", "dangling or cross-request parent"));
            }
        }
        let roots = by_id
            .values()
            .filter(|node| node.parent.is_none())
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
        if roots.len() != 1 {
            return Err(corrupt(
                "reason.roots",
                "missing or extra roots, or a cyclic tree",
            ));
        }
        let root_id = roots
            .into_iter()
            .next()
            .ok_or_else(|| corrupt("reason.root", "missing root"))?;
        let root = by_id
            .remove(&root_id)
            .ok_or_else(|| corrupt("reason.root", "root disappeared"))?;
        if root.ordinal != 0 {
            return Err(corrupt("reason.ordinal", "noncontiguous root ordinal"));
        }
        let root_evidence = self.evidence.remove(&root_id).unwrap_or_default();
        let reason = if root.fields.group == "root" && root.fields.kind == "projectRejected" {
            root.fields.active(&[])?;
            if !root_evidence.is_empty() || by_id.len() != 1 {
                return Err(corrupt(
                    "reason.project_rejected",
                    "wrapper must own exactly one child and no evidence",
                ));
            }
            let (_, child) = by_id
                .pop_first()
                .ok_or_else(|| corrupt("reason.child", "missing project rejection child"))?;
            if child.parent.as_deref() != Some(&root_id) || child.ordinal != 0 {
                return Err(corrupt("reason.child", "wrong child parent or ordinal"));
            }
            let evidence = self.evidence.remove(&child.id).unwrap_or_default();
            Refusal::ProjectRejected {
                reason: Box::new(crate::fleet_refusal_codec::decode(
                    child.fields,
                    evidence,
                    &child.id,
                )?),
            }
        } else {
            if !by_id.is_empty() {
                return Err(corrupt(
                    "reason.nodes",
                    "unreachable, extra or cyclic child nodes",
                ));
            }
            crate::fleet_refusal_codec::decode(root.fields, root_evidence, &root_id)?
        };
        reason
            .validate_bounds()
            .map_err(|error| corrupt("reason", error))?;
        Ok(vec![reason])
    }
    pub(crate) fn finish(self) -> Result<(), StoreError> {
        if !self.nodes.is_empty() || !self.evidence.is_empty() {
            return Err(corrupt(
                "reason.nodes",
                "unreachable nodes or evidence remain",
            ));
        }
        Ok(())
    }
}
