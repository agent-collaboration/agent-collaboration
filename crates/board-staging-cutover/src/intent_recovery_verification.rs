//! Accept only the named, causally linked local emitted-intent handoff progression.
use crate::intent_recovery_feed::{decode_feed, verify_decision_row, verify_intent_row};
use crate::intent_recovery_rows::{
    RecoveryDelta, RecoveryError, RecoveryResult, Row, compare_recovery_rows, integer, json,
    require, text,
};
use crate::{NativeProjectSnapshot, PreparedProjectBundle};
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::{MailSender, NotificationSource};
use fleet_sync_protocol::{DeliveryEnvelope, PeerId};
use message_board::{
    ChangeOutcome, FeedContent, Identity, IntentState, Record, RecordId, RecordKind,
    RecordPlacement, RecordedActor, RouterComponent, ServiceId, StoredDecision,
};
use message_board_storage::MailHandoffItem;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VerifiedIntentRecovery {
    pub handed_off: u64,
    pub remaining_emitted: u64,
}

pub(crate) async fn verify_intent_recovery(
    bundle: &PreparedProjectBundle,
    installed: &mut NativeProjectSnapshot<'_>,
    service: &ServiceId,
    peer: PeerId,
    machine_store: &std::path::Path,
) -> RecoveryResult<VerifiedIntentRecovery> {
    let expected = board_fleet_sync::BoardProjectSchema::new()
        .map_err(|_| RecoveryError::Invalid("schema catalog"))?;
    require(
        expected.supported_schemas().last() == Some(&bundle.manifest().source.manifest.schema),
        "current recovery schema",
    )?;
    let source = &bundle.manifest().source;
    let actual = installed.preflight().clone();
    require(
        actual.project_scope == source.project_scope
            && actual.application_epoch.get() == 2
            && actual.high_water.epoch.get() == 2,
        "recovery project history",
    )?;
    let actor = RecordedActor::Router {
        component: RouterComponent::SubscriptionDelivery,
        installation: service.clone(),
    };
    let actor_json =
        serde_json::to_string(&actor).map_err(|_| RecoveryError::Invalid("actor encoding"))?;
    let delta = compare_recovery_rows(bundle, installed, &actor_json).await?;
    let count =
        u64::try_from(delta.intents.len()).map_err(|_| RecoveryError::Invalid("recovery count"))?;
    require(
        count > 0 && count <= delta.source_emitted,
        "named emitted-intent count",
    )?;
    let feed_count = count
        .checked_mul(2)
        .ok_or(RecoveryError::Invalid("feed count"))?;
    require(
        actual.high_water.position.get()
            == source
                .high_water
                .position
                .get()
                .checked_add(feed_count)
                .ok_or(RecoveryError::Invalid("feed overflow"))?,
        "appended recovery head",
    )?;
    require(
        u64::try_from(delta.added("project_feed").len()).ok() == Some(feed_count)
            && u64::try_from(delta.added("request_decisions").len()).ok() == Some(count)
            && u64::try_from(delta.added("mail_handoffs").len()).ok() == Some(count),
        "extra recovery effects",
    )?;
    let sequences = delta
        .added("mail_handoffs")
        .iter()
        .map(|row| integer(row, 1))
        .collect::<RecoveryResult<BTreeSet<_>>>()?;
    require(
        u64::try_from(sequences.len()).ok() == Some(count),
        "duplicate handoff sequence",
    )?;
    for offset in 1..=count {
        let sequence = delta
            .source_handoff_head
            .checked_add(
                i64::try_from(offset).map_err(|_| RecoveryError::Invalid("handoff range"))?,
            )
            .ok_or(RecoveryError::Invalid("handoff overflow"))?;
        require(sequences.contains(&sequence), "handoff sequence gap")?;
    }
    let actor_id = producer_actor(&delta, &actor_json)?;
    check_line(&delta, source, &actual)?;
    crate::recovery_frontier::verify_frontier(&delta, machine_store, service, peer).await?;
    let causes = source_causes(bundle, &delta)?;
    let mut feed = BTreeMap::new();
    for row in delta.added("project_feed") {
        require(
            feed.insert(integer(row, 2)?, row).is_none(),
            "duplicate recovery position",
        )?;
    }
    for offset in 1..=feed_count {
        let position = source
            .high_water
            .position
            .get()
            .checked_add(offset)
            .ok_or(RecoveryError::Invalid("feed offset"))?;
        require(
            feed.contains_key(
                &i64::try_from(position).map_err(|_| RecoveryError::Invalid("feed range"))?,
            ),
            "recovery feed gap",
        )?;
    }
    let mut decisions = rows_by_id(delta.added("request_decisions"))?;
    let mut handoffs = rows_by_id(delta.added("mail_handoffs"))?;
    let mut last_time = None;
    let mut latest_position = 0_i64;
    for (before, after) in &delta.intents {
        let position = integer(after, 4)?;
        let decision_position = position
            .checked_add(1)
            .ok_or(RecoveryError::Invalid("decision position"))?;
        let intent_feed = feed
            .remove(&position)
            .ok_or(RecoveryError::Invalid("intent feed"))?;
        let decision_feed = feed
            .remove(&decision_position)
            .ok_or(RecoveryError::Invalid("decision feed"))?;
        let Record::NotifyIntent(intent) =
            decode_feed(intent_feed, &actor_id, &actor, &source.project_scope)?
        else {
            return Err(RecoveryError::Invalid("intent progression"));
        };
        let Record::Request(decision) =
            decode_feed(decision_feed, &actor_id, &actor, &source.project_scope)?
        else {
            return Err(RecoveryError::Invalid("intent decision"));
        };
        verify_intent_row(before, after, &intent, &actor_id, intent_feed)?;
        let IntentState::HandedOff { item } = intent.state() else {
            return Err(RecoveryError::Invalid("handoff state"));
        };
        require(
            intent.envelope.request.as_ref() == Some(&decision.request_id)
                && decision.envelope.request.as_ref() == Some(&decision.request_id)
                && intent.envelope.time == decision.envelope.time
                && decision.accepted_in.is_some_and(|epoch| epoch.get() == 2),
            "intent transaction identity",
        )?;
        let expected = StoredDecision::accepted(
            vec![(
                intent.intent_id.clone().into(),
                intent.envelope.position.clone(),
            )],
            ChangeOutcome::Changed,
        );
        require(decision.decision == expected, "intent decision effect")?;
        let row = decisions
            .remove(decision.request_id.as_str())
            .ok_or(RecoveryError::Invalid("decision row missing"))?;
        verify_decision_row(row, &decision, &actor_id, decision_feed)?;
        let row = handoffs
            .remove(item.as_str())
            .ok_or(RecoveryError::Invalid("handoff row missing"))?;
        let cause = causes
            .get(&(intent.cause.kind, intent.cause.id.clone()))
            .ok_or(RecoveryError::Invalid("retained cause missing"))?;
        check_handoff(row, &intent, cause, service, peer, &source.selected.project)?;
        if decision_position > latest_position {
            latest_position = decision_position;
            last_time = Some(decision.envelope.time);
        }
    }
    require(
        feed.is_empty() && decisions.is_empty() && handoffs.is_empty(),
        "unexplained appended row",
    )?;
    require(
        last_time == Some(actual.latest_record_at),
        "recovery latest time",
    )?;
    Ok(VerifiedIntentRecovery {
        handed_off: count,
        remaining_emitted: delta.source_emitted - count,
    })
}

fn producer_actor(delta: &RecoveryDelta, expected: &str) -> RecoveryResult<String> {
    match (&delta.existing_actor, delta.added("actors")) {
        (Some(id), []) => Ok(id.clone()),
        (None, [row]) => {
            require(
                row.len() == 2 && text(row, 1)? == expected,
                "new producer actor",
            )?;
            let id = text(row, 0)?;
            let uuid = uuid::Uuid::parse_str(id)
                .map_err(|_| RecoveryError::Invalid("producer identifier"))?;
            require(runtime_primitives::uuid_v7(uuid), "producer UUIDv7")?;
            Ok(id.to_owned())
        }
        _ => Err(RecoveryError::Invalid("producer actor cardinality")),
    }
}

fn check_line(
    delta: &RecoveryDelta,
    source: &crate::NativeSourcePreflight,
    actual: &crate::NativeSourcePreflight,
) -> RecoveryResult<()> {
    match (&source.line_two, delta.added("epoch_lines")) {
        (Some(previous), []) => require(
            actual.line_two.as_ref() == Some(previous),
            "preserved line two",
        ),
        (None, [row]) => {
            let line = actual
                .line_two
                .as_ref()
                .ok_or(RecoveryError::Invalid("new line two"))?;
            require(
                row.len() == 3
                    && text(row, 0)? == line.line_id.as_str()
                    && integer(row, 1)? == 2
                    && integer(row, 2)?
                        == i64::try_from(source.high_water.position.get())
                            .map_err(|_| RecoveryError::Invalid("inherited range"))?
                    && line.inherited_through == source.high_water.position,
                "inherited source prefix",
            )
        }
        _ => Err(RecoveryError::Invalid("epoch-line cardinality")),
    }
}

fn rows_by_id(rows: &[Row]) -> RecoveryResult<BTreeMap<String, &Row>> {
    let mut indexed = BTreeMap::new();
    for row in rows {
        require(
            indexed.insert(text(row, 0)?.to_owned(), row).is_none(),
            "duplicate effect identity",
        )?;
    }
    Ok(indexed)
}

fn source_causes(
    bundle: &PreparedProjectBundle,
    delta: &RecoveryDelta,
) -> RecoveryResult<BTreeMap<(RecordKind, RecordId), Record>> {
    let mut wanted = BTreeSet::new();
    for (row, _) in &delta.intents {
        let kind: RecordKind =
            serde_json::from_value(serde_json::Value::String(text(row, 7)?.to_owned()))
                .map_err(|_| RecoveryError::Invalid("cause kind"))?;
        let id = RecordId::try_from(text(row, 8)?.to_owned())
            .map_err(|_| RecoveryError::Invalid("cause identifier"))?;
        require(
            !matches!(kind, RecordKind::NotifyIntent | RecordKind::Request),
            "unsupported mutable recovery cause",
        )?;
        wanted.insert((kind, id));
    }
    let mut found: BTreeMap<(RecordKind, RecordId), (i64, Record)> = BTreeMap::new();
    for index in 0..bundle.manifest().header.pages {
        let page = bundle.page(index).map_err(|_| RecoveryError::Read)?;
        if page.table != "project_feed" {
            continue;
        }
        for row in page.rows {
            require(row.len() == 12, "source feed layout")?;
            let kind: RecordKind =
                serde_json::from_value(serde_json::Value::String(text(&row, 3)?.to_owned()))
                    .map_err(|_| RecoveryError::Invalid("source feed kind"))?;
            let id = RecordId::try_from(text(&row, 4)?.to_owned())
                .map_err(|_| RecoveryError::Invalid("source feed identifier"))?;
            if !wanted.contains(&(kind, id.clone())) {
                continue;
            }
            let FeedContent::Shown(record) = json(&row, 11)? else {
                return Err(RecoveryError::Invalid("redacted source cause"));
            };
            require(
                record.reference().kind == kind && record.reference().id == id,
                "source cause reference",
            )?;
            let position = integer(&row, 2)?;
            let RecordPlacement::Project(entry) = &record.envelope().position else {
                return Err(RecoveryError::Invalid("source cause cursor"));
            };
            require(
                i64::try_from(entry.cursor.position.get()).ok() == Some(position)
                    && i64::try_from(entry.cursor.epoch.get()).ok() == Some(integer(&row, 1)?),
                "source cause envelope cursor",
            )?;
            let key = (kind, id);
            if found
                .get(&key)
                .is_none_or(|(previous, _)| position > *previous)
            {
                found.insert(key, (position, *record));
            }
        }
    }
    require(found.len() == wanted.len(), "source cause coverage")?;
    Ok(found
        .into_iter()
        .map(|(key, (_, record))| (key, record))
        .collect())
}

fn check_handoff(
    row: &Row,
    intent: &message_board::NotifyIntent,
    cause: &Record,
    service: &ServiceId,
    peer: PeerId,
    project: &message_board::ProjectId,
) -> RecoveryResult<()> {
    require(row.len() == 3 && integer(row, 1)? > 0, "handoff row layout")?;
    let IntentState::HandedOff { item } = intent.state() else {
        return Err(RecoveryError::Invalid("handoff reference"));
    };
    require(text(row, 0)? == item.as_str(), "handoff identity")?;
    let MailHandoffItem::Delivery(envelope) = json(row, 2)? else {
        return Err(RecoveryError::Invalid("unexpected cancel group"));
    };
    let DeliveryEnvelope::Item(delivery) = *envelope else {
        return Err(RecoveryError::Invalid("unexpected cancel"));
    };
    require(
        delivery.id().as_uuid().to_string() == item.as_str()
            && delivery.sender()
                == &MailSender::Project {
                    project: project.clone(),
                }
            && delivery.valid_until().is_none(),
        "project handoff sender or expiry",
    )?;
    let message_board::IntentRecipient::Resolved(recipient) = intent.recipient() else {
        return Err(RecoveryError::Invalid("unresolved source recipient"));
    };
    let Some(Identity::Session { session }) = recipient.actor.declared_identity() else {
        return Err(RecoveryError::Invalid("non-session recipient"));
    };
    // This first named rule covers the proven local recipient. Remote enrollment
    // attestation is not inferred from a matching address or accepted silently.
    require(
        session.endpoint.service_id == *service
            && delivery.recipient().peer == peer
            && delivery
                .recipient()
                .address
                .session()
                .is_ok_and(|address| address == &session),
        "local recipient enrollment",
    )?;
    delivery
        .body()
        .validate_sender(delivery.sender())
        .map_err(|_| RecoveryError::Invalid("handoff sender validation"))?;
    let BoardDeliveryBody::Notification(body) = delivery.body() else {
        return Err(RecoveryError::Invalid("notification body"));
    };
    require(
        body.pointer.project == *project
            && body.pointer.subject == intent.cause
            && Some(body.pointer.position) == cause.envelope().position.project_cursor()
            && body.source
                == NotificationSource::Intent {
                    intent: intent.intent_id.clone(),
                },
        "retained notification source",
    )?;
    let excerpt = match cause {
        Record::Post(post) => post.text.as_str().chars().take(280).collect::<String>(),
        _ => {
            collaboration_runtime::received_mail_push::board_notification_summary(intent.cause.kind)
                .to_owned()
        }
    };
    require(
        body.excerpt.as_str() == excerpt,
        "source notification excerpt",
    )
}
