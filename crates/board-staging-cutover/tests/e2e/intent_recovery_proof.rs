//! Account for one real intent transaction; never ignore a whole changed table.
use super::{
    offline_cli_fixture::TestResult,
    recovery_rows::{RecoveryRows, Row, cell, integer, json, require, table, text},
};
use board_staging_cutover::{NativeProjectSnapshot, PreparedProjectBundle};
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::{MailSender, NotificationSource};
use fleet_sync_protocol::{DeliveryEnvelope, PeerId};
use message_board::{
    ChangeOutcome, FeedContent, FeedCursor, IntentState, NotifyIntent, Record, RecordId,
    RecordPlacement, RecordedActor, RequestLedgerEntry, RouterComponent, StoredDecision,
};
use message_board_storage::MailHandoffItem;
use std::{collections::BTreeMap, path::Path, time::Duration};

pub(super) struct RecoveredIntent {
    pub head: FeedCursor,
    pub item: message_board::MailItemRef,
}

pub(super) async fn await_recovered_intent_handoff(
    router_root: &Path,
    project: &message_board::ProjectId,
    before: &NotifyIntent,
) -> TestResult {
    let directory = codex_router_host::router_collaboration_directory(router_root);
    let client = collaboration_client::CollaborationClient::connect(
        &directory,
        "cutover-recovery-readback",
        "1",
    )
    .await?;
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let shown = client
                .intent_show(message_board::ProjectRecordShowRequest {
                    project_id: project.clone(),
                    record: before.intent_id.clone().into(),
                })
                .await?;
            let Record::NotifyIntent(observed) = shown else {
                return Err("source recovery intent returned another record kind".into());
            };
            match observed.state() {
                IntentState::HandedOff { .. } => return TestResult::Ok(()),
                IntentState::Emitted {} => {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                state @ (IntentState::Superseded {} | IntentState::Refused { .. }) => {
                    return Err(
                        format!("source recovery intent did not hand off: {state:?}").into(),
                    );
                }
            }
        }
    })
    .await
    .map_err(|_| std::io::Error::other("source recovery intent stayed Emitted before Host stop"))?
}

pub(super) async fn verify_recovered_intent(
    bundle: &PreparedProjectBundle,
    installed: &mut NativeProjectSnapshot<'_>,
    before: &NotifyIntent,
    peer: PeerId,
) -> TestResult<RecoveredIntent> {
    let initial = &bundle.manifest().source;
    let head = installed.preflight().high_water;
    require(
        head.epoch.get() == 2 && head.position.get() == initial.high_water.position.get() + 2,
        "recovery feed did not append exactly two entries",
    )?;
    let rows = RecoveryRows::read(bundle, installed).await?;
    let RecordPlacement::Project(original) = &before.envelope.position else {
        return Err("source intent is not project-scoped".into());
    };
    let actor = RecordedActor::Router {
        component: RouterComponent::SubscriptionDelivery,
        installation: original.origin.installation.clone(),
    };
    let added_actors = rows.added("actors")?;
    let [actor_row] = added_actors.as_slice() else {
        return Err("recovery did not add exactly one named actor".into());
    };
    require(actor_row.len() == 2, "unexpected actor layout")?;
    let actor_id = RecordId::try_from(text(actor_row, 0)?.to_owned())?;
    require(
        text(actor_row, 1)? == serde_json::to_string(&actor)?,
        "new actor is not the subscription-delivery producer",
    )?;
    let added_lines = rows.added("epoch_lines")?;
    let [line] = added_lines.as_slice() else {
        return Err("recovery did not add exactly one epoch line".into());
    };
    require(line.len() == 3, "unexpected epoch-line layout")?;
    let line_two = installed
        .preflight()
        .line_two
        .as_ref()
        .ok_or("recovery line two missing")?;
    require(
        text(line, 0)? == line_two.line_id.as_str()
            && integer(line, 1)? == 2
            && integer(line, 2)? == i64::try_from(initial.high_water.position.get())?,
        "recovery changed the inherited epoch boundary",
    )?;
    let added_feed = rows.added("project_feed")?;
    let mut feed = BTreeMap::new();
    for row in added_feed {
        require(
            feed.insert(integer(row, 2)?, row).is_none(),
            "duplicate recovery feed position",
        )?;
    }
    let intent_position = i64::try_from(initial.high_water.position.get() + 1)?;
    let decision_position = intent_position + 1;
    let intent_feed = feed
        .remove(&intent_position)
        .ok_or("intent progression missing from feed")?;
    let decision_feed = feed
        .remove(&decision_position)
        .ok_or("decision progression missing from feed")?;
    require(feed.is_empty(), "extra recovery feed row")?;
    let Record::NotifyIntent(after) = decode_feed(intent_feed, &actor_id, &actor, intent_position)?
    else {
        return Err("first recovery feed entry is not the intent".into());
    };
    let Record::Request(decision) =
        decode_feed(decision_feed, &actor_id, &actor, decision_position)?
    else {
        return Err("second recovery feed entry is not its request decision".into());
    };
    require(
        after.intent_id == before.intent_id
            && after.cause == before.cause
            && after.recipient() == before.recipient(),
        "recovery lost or retargeted the original intent",
    )?;
    require(
        after.envelope.scope == before.envelope.scope,
        "recovery changed the intent scope",
    )?;
    let IntentState::HandedOff { item } = after.state() else {
        return Err("emitted intent was not handed off once".into());
    };
    require(
        after.envelope.request.as_ref() == Some(&decision.request_id),
        "intent and decision request identities differ",
    )?;
    require(
        decision.envelope.request.as_ref() == Some(&decision.request_id),
        "decision lost its own request identity",
    )?;
    require(
        decision.envelope.time == after.envelope.time
            && decision.envelope.scope == after.envelope.scope,
        "intent and decision are not one transaction",
    )?;
    require(
        decision.accepted_in.map(|epoch| epoch.get()) == Some(2),
        "recovery decision names another writer epoch",
    )?;
    let expected = StoredDecision::accepted(
        vec![(
            after.intent_id.clone().into(),
            after.envelope.position.clone(),
        )],
        ChangeOutcome::Changed,
    );
    require(
        decision.decision == expected,
        "recovery decision describes another effect",
    )?;
    check_intent_row(&rows, before, &after, &actor_id, intent_feed)?;
    check_decision_row(&rows, &decision, &actor_id, decision_feed)?;
    let handoffs = table(&rows.after, "mail_handoffs")?;
    let [handoff] = handoffs else {
        return Err("recovery did not allocate exactly one handoff".into());
    };
    require(
        handoff.len() == 3 && text(handoff, 0)? == item.as_str() && integer(handoff, 1)? == 1,
        "handoff identity or ordinal differs from the intent",
    )?;
    let MailHandoffItem::Delivery(envelope) = json(handoff, 2)? else {
        return Err("intent created another handoff kind".into());
    };
    let DeliveryEnvelope::Item(delivery) = *envelope else {
        return Err("intent created a cancel instead of a notification".into());
    };
    require(
        delivery.id().as_uuid().to_string() == item.as_str(),
        "handoff body and retained item identity differ",
    )?;
    let project = &initial.selected.project;
    require(
        delivery.sender()
            == &MailSender::Project {
                project: project.clone(),
            },
        "handoff lost its project sender",
    )?;
    require(
        delivery.recipient().peer == peer && delivery.valid_until().is_none(),
        "handoff changed recipient peer or expiry",
    )?;
    let message_board::IntentRecipient::Resolved(recipient) = before.recipient() else {
        return Err("source recipient is unresolved".into());
    };
    let Some(message_board::Identity::Session { session }) = recipient.actor.declared_identity()
    else {
        return Err("source recipient is not a declared session".into());
    };
    require(
        delivery
            .recipient()
            .address
            .session()
            .is_ok_and(|address| address == &session),
        "handoff changed the recipient address",
    )?;
    let BoardDeliveryBody::Notification(body) = delivery.body() else {
        return Err("handoff is not a notification".into());
    };
    require(
        body.pointer.project == *project && body.pointer.subject == before.cause,
        "handoff changed its cause pointer",
    )?;
    require(
        body.source
            == NotificationSource::Intent {
                intent: before.intent_id.clone(),
            },
        "handoff lost its source intent",
    )?;
    require(
        body.excerpt.as_str() == "New board activity",
        "handoff has an unexplained excerpt",
    )?;
    let original_feed = table(&rows.before, "project_feed")?
        .iter()
        .find(|row| text(row, 4).is_ok_and(|id| id == before.cause.id.as_str()))
        .ok_or("original cause feed entry missing")?;
    let FeedContent::Shown(cause) = json(original_feed, 11)? else {
        return Err("original cause is redacted".into());
    };
    require(
        Some(body.pointer.position) == cause.envelope().position.project_cursor(),
        "handoff changed the retained cause cursor",
    )?;
    Ok(RecoveredIntent {
        head,
        item: item.clone(),
    })
}

fn decode_feed(
    row: &Row,
    actor_id: &RecordId,
    actor: &RecordedActor,
    position: i64,
) -> TestResult<Record> {
    require(row.len() == 12, "unexpected project-feed layout")?;
    RecordId::try_from(text(row, 0)?.to_owned())?;
    let FeedContent::Shown(record) = json(row, 11)? else {
        return Err("recovery feed entry is redacted".into());
    };
    let reference = record.reference();
    require(
        record.envelope().id == reference.id,
        "feed content has another envelope identity",
    )?;
    let tag: message_board::RecordKind =
        serde_json::from_value(serde_json::Value::String(text(row, 3)?.to_owned()))?;
    require(
        tag == reference.kind && text(row, 4)? == reference.id.as_str(),
        "feed record reference differs from its content",
    )?;
    require(
        text(row, 6)? == actor_id.as_str() && record.envelope().actor == *actor,
        "feed actor is unexplained",
    )?;
    let envelope = record.envelope();
    let request = envelope
        .request
        .as_ref()
        .ok_or("recovery request missing")?;
    require(
        text(row, 5)? == request.as_str(),
        "feed request differs from content",
    )?;
    let RecordPlacement::Project(entry) = &envelope.position else {
        return Err("recovery is not project-scoped".into());
    };
    require(
        integer(row, 1)? == 2
            && integer(row, 2)? == position
            && entry.cursor.epoch.get() == 2
            && i64::try_from(entry.cursor.position.get())? == position,
        "recovery feed cursor differs",
    )?;
    require(
        text(row, 7)? == entry.origin.installation.as_str()
            && text(row, 8)? == entry.origin.machine.as_str(),
        "recovery origin differs from content",
    )?;
    let RecordedActor::Router { installation, .. } = actor else {
        return Err("expected recovery actor is not Router-owned".into());
    };
    require(
        &entry.origin.installation == installation,
        "recovery originated on another installation",
    )?;
    let message_board::RecordTime::Router(time) = envelope.time else {
        return Err("recovery time is not Router-owned".into());
    };
    require(
        chrono::DateTime::parse_from_rfc3339(text(row, 9)?)?.with_timezone(&chrono::Utc) == time
            && text(row, 10)? == "router",
        "recovery time differs from content",
    )?;
    Ok(*record)
}

fn check_intent_row(
    rows: &RecoveryRows,
    before: &NotifyIntent,
    after: &NotifyIntent,
    actor: &RecordId,
    feed: &Row,
) -> TestResult {
    let [source] = table(&rows.before, "notify_intents")? else {
        return Err("source intent count differs".into());
    };
    let [target] = table(&rows.after, "notify_intents")? else {
        return Err("installed intent count differs".into());
    };
    require(
        source.len() == 12 && target.len() == 12,
        "unexpected intent layout",
    )?;
    for index in [0, 7, 8, 9, 10] {
        require(
            cell(source, index)? == cell(target, index)?,
            "recovery changed an immutable intent field",
        )?;
    }
    require(
        text(target, 0)? == before.intent_id.as_str()
            && text(target, 1)? == actor.as_str()
            && cell(target, 2)? == cell(feed, 5)?
            && integer(target, 3)? == 2
            && cell(target, 4)? == cell(feed, 2)?
            && cell(target, 5)? == cell(feed, 9)?
            && cell(target, 6)? == cell(feed, 10)?,
        "intent envelope does not match its feed progression",
    )?;
    require(
        json::<IntentState>(source, 11)? == IntentState::Emitted {}
            && json::<IntentState>(target, 11)? == *after.state(),
        "intent state does not match the single handoff",
    )?;
    Ok(())
}
fn check_decision_row(
    rows: &RecoveryRows,
    decision: &RequestLedgerEntry,
    actor: &RecordId,
    feed: &Row,
) -> TestResult {
    let [row] = table(&rows.after, "request_decisions")? else {
        return Err("recovery decision count differs".into());
    };
    require(row.len() == 9, "unexpected decision layout")?;
    require(
        text(row, 0)? == decision.request_id.as_str()
            && text(row, 1)? == actor.as_str()
            && integer(row, 2)? == 2
            && cell(row, 3)? == cell(feed, 2)?
            && cell(row, 4)? == cell(feed, 9)?
            && cell(row, 5)? == cell(feed, 10)?
            && text(row, 6)? == decision.canonical_hash.as_str()
            && integer(row, 7)? == 2
            && json::<StoredDecision>(row, 8)? == decision.decision,
        "decision row differs from its recorded effect",
    )
}
