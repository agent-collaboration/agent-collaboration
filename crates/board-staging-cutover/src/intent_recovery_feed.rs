//! Validate each appended recovery record against its stored envelope and decision row.
use crate::intent_recovery_rows::{
    RecoveryError, RecoveryResult, Row, cell, integer, json, require, text,
};
use message_board::{
    FeedContent, IntentState, NotifyIntent, Record, RecordId, RecordPlacement, RecordScope,
    RecordedActor, RequestLedgerEntry,
};

pub(crate) fn decode_feed(
    row: &Row,
    actor_id: &str,
    actor: &RecordedActor,
    scope: &RecordScope,
) -> RecoveryResult<Record> {
    require(row.len() == 12, "feed layout")?;
    RecordId::try_from(text(row, 0)?.to_owned())
        .map_err(|_| RecoveryError::Invalid("feed identity"))?;
    let FeedContent::Shown(record) = json(row, 11)? else {
        return Err(RecoveryError::Invalid("recovery is redacted"));
    };
    let reference = record.reference();
    let tag: message_board::RecordKind =
        serde_json::from_value(serde_json::Value::String(text(row, 3)?.to_owned()))
            .map_err(|_| RecoveryError::Invalid("feed kind"))?;
    require(
        tag == reference.kind
            && text(row, 4)? == reference.id.as_str()
            && record.envelope().id == reference.id,
        "feed reference",
    )?;
    let envelope = record.envelope();
    require(
        envelope.scope == *scope && envelope.actor == *actor && text(row, 6)? == actor_id,
        "feed scope or actor",
    )?;
    let request = envelope
        .request
        .as_ref()
        .ok_or(RecoveryError::Invalid("feed request"))?;
    require(text(row, 5)? == request.as_str(), "feed request identity")?;
    let RecordPlacement::Project(entry) = &envelope.position else {
        return Err(RecoveryError::Invalid("feed placement"));
    };
    require(
        integer(row, 1)? == 2
            && entry.cursor.epoch.get() == 2
            && integer(row, 2)?
                == i64::try_from(entry.cursor.position.get())
                    .map_err(|_| RecoveryError::Invalid("feed position"))?,
        "feed cursor",
    )?;
    let RecordedActor::Router { installation, .. } = actor else {
        return Err(RecoveryError::Invalid("producer actor"));
    };
    require(
        entry.origin.installation == *installation
            && text(row, 7)? == entry.origin.installation.as_str()
            && text(row, 8)? == entry.origin.machine.as_str(),
        "feed origin",
    )?;
    let message_board::RecordTime::Router(time) = envelope.time else {
        return Err(RecoveryError::Invalid("feed time basis"));
    };
    let stored = chrono::DateTime::parse_from_rfc3339(text(row, 9)?)
        .map_err(|_| RecoveryError::Invalid("feed time"))?
        .with_timezone(&chrono::Utc);
    require(stored == time && text(row, 10)? == "router", "feed time")?;
    Ok(*record)
}

pub(crate) fn verify_intent_row(
    before: &Row,
    after: &Row,
    intent: &NotifyIntent,
    actor: &str,
    feed: &Row,
) -> RecoveryResult<()> {
    require(before.len() == 12 && after.len() == 12, "intent layout")?;
    for index in [0, 7, 8, 9, 10] {
        require(
            cell(before, index)? == cell(after, index)?,
            "immutable intent field",
        )?;
    }
    require(
        json::<IntentState>(before, 11)? == IntentState::Emitted {},
        "source intent is not emitted",
    )?;
    require(
        json::<IntentState>(after, 11)? == *intent.state(),
        "installed intent state",
    )?;
    require(
        text(after, 0)? == intent.intent_id.as_str()
            && text(after, 1)? == actor
            && cell(after, 2)? == cell(feed, 5)?
            && integer(after, 3)? == 2
            && cell(after, 4)? == cell(feed, 2)?
            && cell(after, 5)? == cell(feed, 9)?
            && cell(after, 6)? == cell(feed, 10)?,
        "intent row and feed differ",
    )?;
    let kind: message_board::RecordKind =
        serde_json::from_value(serde_json::Value::String(text(before, 7)?.to_owned()))
            .map_err(|_| RecoveryError::Invalid("cause kind"))?;
    require(
        kind == intent.cause.kind
            && text(before, 8)? == intent.cause.id.as_str()
            && json::<message_board::IntentRecipient>(before, 10)? == *intent.recipient(),
        "retained intent cause or recipient",
    )
}

pub(crate) fn verify_decision_row(
    row: &Row,
    decision: &RequestLedgerEntry,
    actor: &str,
    feed: &Row,
) -> RecoveryResult<()> {
    require(row.len() == 9, "decision layout")?;
    require(
        text(row, 0)? == decision.request_id.as_str()
            && text(row, 1)? == actor
            && integer(row, 2)? == 2
            && cell(row, 3)? == cell(feed, 2)?
            && cell(row, 4)? == cell(feed, 9)?
            && cell(row, 5)? == cell(feed, 10)?
            && text(row, 6)? == decision.canonical_hash.as_str()
            && integer(row, 7)? == 2
            && json::<message_board::StoredDecision>(row, 8)? == decision.decision,
        "decision row and feed differ",
    )
}
