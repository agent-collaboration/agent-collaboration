//! Recording a case's messages, each at its named layer, and checking them when the case
//! finishes (D0 Program Design "Recording: layers, complete captures, intent").
//!
//! Every recording call takes a [`RecordLayer`]. A wire-frame or body record keeps its raw value
//! beside the stable copy the case compares; typed records are counted and never checked. When
//! the case finishes, every wire and body record is checked in order through one connection
//! ledger, so each reply pairs with the request recorded before it on its own connection.

use std::collections::HashMap;

use serde_json::Value;

use super::connection_ledger::{AcpPeer, ConnectionId, ConnectionLedger};
use super::edge_conformance::{
    CheckInput, EdgeConformance, EdgeDefinitions, OwnedCodec, OwnedCodecs, OwnedVerdicts,
    RecordLayer, RouterContract, Violation, family_edge, sender_role,
};
use super::expected_failures::SchemaDefect;
use super::schema_settlement::{self, Observed, RecordIntent, SchemaSettlement, direction_label};
use super::{EdgeParty, MessageFamily, TapCase, TapDirection, TapRecord, stable};

/// The fields of one record, before it is pushed.
struct NewRecord<'a> {
    direction: TapDirection,
    family: MessageFamily,
    counterpart: EdgeParty,
    kind: &'a str,
    reply: bool,
    layer: RecordLayer,
    payload: Option<Value>,
    raw: Option<Value>,
}

impl TapCase {
    /// Names the connection the following records travel on (default `case`).
    pub fn connection(&mut self, name: &str) {
        self.connection = ConnectionId(name.to_owned());
    }

    /// Declares which ACP agent the current connection talks to (Claude or Cursor): it decides
    /// who owns the connection's extension methods.
    pub fn acp_peer(&mut self, peer: AcpPeer) {
        self.acp_peers.push((self.connection.clone(), peer));
    }

    /// Holds this case's Router-owned contracts to `codecs`: the shared registry of owning checks
    /// (`OwnedCodecs::from_owner_registry` over `acp_provider_adapters::owned_contract_checks`) the live checker uses too.
    pub fn use_owned_codecs(&mut self, codecs: OwnedCodecs) {
        self.codecs = codecs;
    }

    /// Holds this case's records of a Router-owned `contract` to its owning codec.
    pub fn register_codec(&mut self, contract: RouterContract, codec: OwnedCodec) {
        self.codecs.register(contract, codec);
    }

    /// Holds this case's records of `contract` to the agreement of its owning Zod verdict and its
    /// Rust decoder's typed outcome (model metadata has a decoder and no encoder).
    pub fn register_verdicts(&mut self, contract: RouterContract, verdicts: OwnedVerdicts) {
        self.codecs.register_verdicts(contract, verdicts);
    }

    /// Declares that records of `kind` cannot carry their raw value here, and why. Their gaps
    /// are listed, not failed.
    pub fn declare_coverage_gap(&mut self, kind: &str, reason: &str) {
        self.declared_gaps
            .push((kind.to_owned(), reason.to_owned()));
    }

    /// Marks what the last record is: a deliberately injected invalid message, or apparatus.
    ///
    /// # Panics
    /// When nothing has been recorded yet.
    #[expect(
        clippy::panic,
        reason = "Marking an absent record is a test-authoring error."
    )]
    pub fn mark_last(&mut self, intent: RecordIntent) {
        match self.records.last_mut() {
            Some(record) => record.intent = intent,
            None => panic!("{}: mark_last before any record", self.case_id()),
        }
    }

    /// Classifies the last record as fixture setup on a complete capture: it is still checked and
    /// counted, but the catalog row compares only the case's own view.
    ///
    /// # Panics
    /// When nothing has been recorded yet.
    #[expect(
        clippy::panic,
        reason = "Marking an absent record is a test-authoring error."
    )]
    pub fn mark_last_setup(&mut self) {
        match self.records.last_mut() {
            Some(record) => record.setup = true,
            None => panic!("{}: mark_last_setup before any record", self.case_id()),
        }
    }

    /// Says which method the last record, a reply, answers: for a case that starts midway.
    ///
    /// # Panics
    /// When nothing has been recorded yet.
    #[expect(
        clippy::panic,
        reason = "Marking an absent record is a test-authoring error."
    )]
    pub fn last_answers(&mut self, method: &str) {
        match self.records.last_mut() {
            Some(record) => record.answers = Some(method.to_owned()),
            None => panic!("{}: last_answers before any record", self.case_id()),
        }
    }

    pub fn consumed(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
    ) {
        self.push(self.new_record(
            TapDirection::Consumed,
            family,
            counterpart,
            kind,
            false,
            layer,
        ));
    }

    pub fn produced(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
    ) {
        self.push(self.new_record(
            TapDirection::Produced,
            family,
            counterpart,
            kind,
            false,
            layer,
        ));
    }

    /// Records a reply the node took in, on the edge of a request it sent; `family` is the
    /// request's row.
    pub fn consumed_reply(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
    ) {
        self.push(self.new_record(
            TapDirection::Consumed,
            family,
            counterpart,
            kind,
            true,
            layer,
        ));
    }

    /// Records a reply the node sent back on the edge of a request it received; `family` is the
    /// request's row.
    pub fn produced_reply(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
    ) {
        self.push(self.new_record(
            TapDirection::Produced,
            family,
            counterpart,
            kind,
            true,
            layer,
        ));
    }

    /// [`Self::consumed`] with the decoded payload the case asserts on.
    pub fn consumed_with(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
        payload: Value,
    ) {
        let mut record = self.new_record(
            TapDirection::Consumed,
            family,
            counterpart,
            kind,
            false,
            layer,
        );
        record.payload = Some(payload);
        self.push(record);
    }

    /// [`Self::produced`] with the decoded payload the case asserts on.
    pub fn produced_with(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
        payload: Value,
    ) {
        let mut record = self.new_record(
            TapDirection::Produced,
            family,
            counterpart,
            kind,
            false,
            layer,
        );
        record.payload = Some(payload);
        self.push(record);
    }

    /// [`Self::consumed_reply`] with its decoded payload.
    pub fn consumed_reply_with(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
        payload: Value,
    ) {
        let mut record = self.new_record(
            TapDirection::Consumed,
            family,
            counterpart,
            kind,
            true,
            layer,
        );
        record.payload = Some(payload);
        self.push(record);
    }

    /// [`Self::produced_reply`] with its decoded payload.
    pub fn produced_reply_with(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
        payload: Value,
    ) {
        let mut record = self.new_record(
            TapDirection::Produced,
            family,
            counterpart,
            kind,
            true,
            layer,
        );
        record.payload = Some(payload);
        self.push(record);
    }

    /// Records a message the node took in exactly as it crossed: `raw` is kept and checked, and
    /// its [`stable`] copy (fields named in `volatile` replaced) is the payload the row compares.
    pub fn consumed_raw(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
        raw: Value,
        volatile: &[&str],
    ) {
        self.push_raw(
            TapDirection::Consumed,
            family,
            counterpart,
            kind,
            false,
            layer,
            raw,
            volatile,
        );
    }

    /// [`Self::consumed_raw`] for a message the node sent out.
    pub fn produced_raw(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
        raw: Value,
        volatile: &[&str],
    ) {
        self.push_raw(
            TapDirection::Produced,
            family,
            counterpart,
            kind,
            false,
            layer,
            raw,
            volatile,
        );
    }

    /// [`Self::consumed_raw`] for a reply the node took in on the edge of a request it sent.
    pub fn consumed_reply_raw(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
        raw: Value,
        volatile: &[&str],
    ) {
        self.push_raw(
            TapDirection::Consumed,
            family,
            counterpart,
            kind,
            true,
            layer,
            raw,
            volatile,
        );
    }

    /// [`Self::consumed_raw`] for a reply the node sent back on the edge of a request it got.
    pub fn produced_reply_raw(
        &mut self,
        layer: RecordLayer,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
        raw: Value,
        volatile: &[&str],
    ) {
        self.push_raw(
            TapDirection::Produced,
            family,
            counterpart,
            kind,
            true,
            layer,
            raw,
            volatile,
        );
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Shared body of the four raw recorders; each argument is a record field."
    )]
    fn push_raw(
        &mut self,
        direction: TapDirection,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &str,
        reply: bool,
        layer: RecordLayer,
        raw: Value,
        volatile: &[&str],
    ) {
        let mut record = self.new_record(direction, family, counterpart, kind, reply, layer);
        record.payload = Some(stable(raw.clone(), volatile));
        record.raw = Some(raw);
        self.push(record);
    }

    fn new_record<'a>(
        &self,
        direction: TapDirection,
        family: MessageFamily,
        counterpart: EdgeParty,
        kind: &'a str,
        reply: bool,
        layer: RecordLayer,
    ) -> NewRecord<'a> {
        NewRecord {
            direction,
            family,
            counterpart,
            kind,
            reply,
            layer,
            payload: None,
            raw: None,
        }
    }

    fn push(&mut self, record: NewRecord<'_>) {
        self.records.push(TapRecord {
            direction: record.direction,
            family: record.family,
            counterpart: record.counterpart,
            reply: record.reply,
            kind: record.kind.to_owned(),
            layer: record.layer,
            connection: self.connection.clone(),
            intent: RecordIntent::Product,
            payload: record.payload,
            raw: record.raw,
            answers: None,
            conformance: None,
            setup: false,
        });
    }

    /// Checks every wire and body record against its definition, in order through one ledger,
    /// and settles what they show against `listed`.
    pub(super) fn check_records(&mut self, listed: &[SchemaDefect]) -> SchemaSettlement {
        let mut ledger = ConnectionLedger::default();
        for (connection, peer) in &self.acp_peers {
            ledger.declare_peer(connection, *peer);
        }
        let mut observed = Observed::default();
        let mut ordinals: HashMap<(TapDirection, String), u16> = HashMap::new();
        let mut declared = Vec::new();
        for record in &mut self.records {
            let ordinal_slot = ordinals
                .entry((record.direction, record.kind.clone()))
                .or_insert(0);
            let ordinal = *ordinal_slot;
            *ordinal_slot += 1;
            let label = format!(
                "{} {} #{ordinal}",
                direction_label(record.direction),
                record.kind
            );
            let declared_reason = self
                .declared_gaps
                .iter()
                .find(|(kind, _)| kind == &record.kind)
                .map(|(_, reason)| reason.clone());
            let mut gap = |problem: String| match &declared_reason {
                Some(reason) => declared.push(format!("{label}: {problem} (declared: {reason})")),
                None => observed.coverage_gaps.push(format!("{label}: {problem}")),
            };
            if matches!(
                record.layer,
                RecordLayer::TypedInput | RecordLayer::TypedOutcome
            ) {
                record.conformance = Some(EdgeConformance::NotSchemaChecked {
                    layer: record.layer.clone(),
                });
                continue;
            }
            let Some(raw) = &record.raw else {
                gap(format!("a {:?} record without its raw value", record.layer));
                continue;
            };
            let Some((edge, part_is_server)) = family_edge(record.family, self.node) else {
                gap(format!(
                    "a {:?} record on the internal family {:?}",
                    record.layer, record.family
                ));
                continue;
            };
            let conformance = EdgeDefinitions::shared().check(
                &CheckInput {
                    edge,
                    connection: &record.connection,
                    sender: sender_role(part_is_server, record.direction),
                    layer: &record.layer,
                    answers: record.answers.as_deref(),
                    raw,
                    codecs: &self.codecs,
                },
                &mut ledger,
            );
            // Every component of a frame counts: a composite's violations and missing checks are
            // all recorded, none hidden by another.
            let violations: Vec<Violation> = conformance
                .leaves()
                .filter_map(|leaf| match leaf {
                    EdgeConformance::Violations { violations, .. } => Some(violations.iter()),
                    _ => None,
                })
                .flatten()
                .cloned()
                .collect();
            for leaf in conformance.leaves() {
                if let EdgeConformance::Incomplete { missing, .. } = leaf {
                    gap(format!("cannot be judged: {missing:?}"));
                }
            }
            // A deliberately invalid record that conforms did not inject its violation.
            if !violations.is_empty() || record.intent != RecordIntent::Product {
                observed.record_violations(
                    record.direction,
                    &record.kind,
                    ordinal,
                    &record.intent,
                    &violations,
                );
            }
            record.conformance = Some(conformance);
        }
        schema_settlement::settle(observed, listed, declared, ledger.unanswered())
    }
}

#[cfg(test)]
#[path = "recording_test.rs"]
mod tests;
