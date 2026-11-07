//! Scenario-specific row assertions use the common joined-Host readback owner.
use super::{owned_host::StoppedHost, records_reopen::stopped_database_values};
use board_sync_protocol::BoardDeliveryTypes;
use fleet_protocol::{DeliveryEnvelope, ItemId, ReplicationPosition};
use message_board_storage::MailHandoffItem;
use router_peer_test_support::FixtureResult;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct CaptureMark {
    position: ReplicationPosition,
    first: Option<u64>,
    last: Option<u64>,
}

pub async fn assert_local_post_at_cursor(
    stopped: &StoppedHost,
    project: &str,
    cursor: message_board::FeedCursor,
) -> FixtureResult {
    let path = PathBuf::from(format!("projects/{project}/e1.sqlite"));
    let rows = stopped_database_values(
        stopped,
        &path,
        "SELECT json_object('epoch',epoch,'position',position) AS value FROM posts",
    )
    .await?;
    let positions = rows
        .iter()
        .map(|row| serde_json::from_str::<message_board::FeedCursor>(row))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        positions
            .iter()
            .filter(|position| **position == cursor)
            .count(),
        1,
        "crashed owner must retain the exact committed post cursor"
    );
    Ok(())
}

pub async fn contains_post_at(
    stopped: &StoppedHost,
    project: &str,
    epoch: u64,
    cursor: message_board::FeedCursor,
) -> FixtureResult<bool> {
    let path = PathBuf::from(format!("projects/{project}/e{epoch}.sqlite"));
    let rows = stopped_database_values(
        stopped,
        &path,
        "SELECT json_object('epoch',epoch,'position',position) AS value FROM posts",
    )
    .await?;
    let positions = rows
        .iter()
        .map(|row| serde_json::from_str::<message_board::FeedCursor>(row))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(positions.contains(&cursor))
}

pub async fn cause_capture(
    stopped: &StoppedHost,
    project: &str,
    cursor: message_board::FeedCursor,
) -> FixtureResult<ReplicationPosition> {
    optional_cause_capture(stopped, project, cursor)
        .await?
        .ok_or_else(|| "post has no owning native capture assignment".into())
}

pub async fn optional_cause_capture(
    stopped: &StoppedHost,
    project: &str,
    cursor: message_board::FeedCursor,
) -> FixtureResult<Option<ReplicationPosition>> {
    let path = PathBuf::from(format!("projects/{project}/e1.sqlite"));
    let rows = stopped_database_values(stopped, &path,
        "SELECT json_object('position',json_object('epoch',epoch,'sequence',captured_position),'first',first_feed_position,'last',last_feed_position) AS value FROM capture_feed_marks ORDER BY epoch,captured_position").await?;
    let matching = rows
        .iter()
        .map(|row| serde_json::from_str::<CaptureMark>(row))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|mark| {
            mark.position.epoch.value() == cursor.epoch.get()
                && mark
                    .first
                    .is_some_and(|first| first <= cursor.position.get())
                && mark.last.is_some_and(|last| last >= cursor.position.get())
        })
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [] => Ok(None),
        [mark] => Ok(Some(mark.position)),
        _ => Err("post has multiple owning native capture assignments".into()),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MembershipContinuation {
    membership: message_board::RecordId,
    generation: message_board::SubscriptionGeneration,
    activated_at: message_board::FeedCursor,
    ended: Option<message_board::MembershipEnd>,
}

pub async fn assert_ongoing_membership(
    stopped: &StoppedHost,
    project: &str,
    epoch: u64,
    original: &message_board::SubscriptionMembership,
) -> FixtureResult {
    let path = PathBuf::from(format!("projects/{project}/e{epoch}.sqlite"));
    let rows = stopped_database_values(stopped, &path,
        "SELECT json_object('membership',membership_id,'generation',generation,'activatedAt',json_object('epoch',activated_epoch,'position',activated_position),'ended',json(ended)) AS value FROM subscription_memberships").await?;
    let matching = rows
        .iter()
        .map(|row| serde_json::from_str::<MembershipContinuation>(row))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|row| row.membership == original.envelope.id)
        .collect::<Vec<_>>();
    let [membership] = matching.as_slice() else {
        return Err("successor has no unique original membership".into());
    };
    assert_eq!(membership.generation, original.generation());
    assert_eq!(membership.activated_at, original.activated_at());
    assert!(membership.ended.is_none());
    Ok(())
}

#[derive(Deserialize)]
struct HandoffCapture {
    item: message_board::MailItemRef,
    position: ReplicationPosition,
}

pub async fn handoff_capture(
    stopped: &StoppedHost,
    project: &str,
    item: ItemId,
) -> FixtureResult<ReplicationPosition> {
    identity_capture(stopped, project, item,
        "SELECT json_object('item',h.item_id,'position',json_object('epoch',m.epoch,'sequence',m.captured_position)) AS value FROM mail_handoffs h JOIN capture_feed_marks m ON m.max_handoff_seq=h.handoff_seq ORDER BY h.handoff_seq,m.captured_position").await
}

pub async fn expansion_capture(
    stopped: &StoppedHost,
    project: &str,
    item: ItemId,
) -> FixtureResult<ReplicationPosition> {
    identity_capture(stopped, project, item,
        "SELECT json_object('item',c.cancel_id,'position',json_object('epoch',m.epoch,'sequence',m.captured_position)) AS value FROM mail_cancel_expansions c JOIN capture_feed_marks m ON m.max_expansion_seq=c.expansion_seq ORDER BY c.expansion_seq,m.captured_position").await
}

async fn identity_capture(
    stopped: &StoppedHost,
    project: &str,
    item: ItemId,
    statement: &'static str,
) -> FixtureResult<ReplicationPosition> {
    let path = PathBuf::from(format!("projects/{project}/e1.sqlite"));
    let rows = stopped_database_values(stopped, &path, statement).await?;
    let identity = item.as_uuid().to_string();
    let matching = rows
        .iter()
        .map(|row| serde_json::from_str::<HandoffCapture>(row))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|capture| capture.item.as_str() == identity)
        .collect::<Vec<_>>();
    let [capture] = matching.as_slice() else {
        return Err("frozen item has no unique owning native capture assignment".into());
    };
    Ok(capture.position)
}

pub async fn handoffs(
    stopped: &StoppedHost,
    project: &str,
    epoch: u64,
) -> FixtureResult<Vec<MailHandoffItem>> {
    let path = PathBuf::from(format!("projects/{project}/e{epoch}.sqlite"));
    stopped_database_values(
        stopped,
        &path,
        "SELECT item AS value FROM mail_handoffs ORDER BY handoff_seq",
    )
    .await?
    .iter()
    .map(|row| serde_json::from_str(row).map_err(Into::into))
    .collect()
}

pub fn deliveries(handoffs: Vec<MailHandoffItem>) -> Vec<DeliveryEnvelope<BoardDeliveryTypes>> {
    handoffs
        .into_iter()
        .filter_map(|handoff| match handoff {
            MailHandoffItem::Delivery(item) => Some(*item),
            MailHandoffItem::CancelGroup(_) => None,
        })
        .collect()
}

pub async fn cancel_expansions(
    stopped: &StoppedHost,
    project: &str,
    epoch: u64,
) -> FixtureResult<Vec<serde_json::Value>> {
    let path = PathBuf::from(format!("projects/{project}/e{epoch}.sqlite"));
    stopped_database_values(stopped, &path,
        "SELECT json_object('cancel',cancel_id,'group',group_handoff,'target',target_item,'envelope',json(envelope)) AS value FROM mail_cancel_expansions ORDER BY expansion_seq")
        .await?.iter().map(|row| serde_json::from_str(row).map_err(Into::into)).collect()
}

pub async fn assert_no_wake(stopped: &StoppedHost) -> FixtureResult {
    let rows = stopped_database_values(
        stopped,
        Path::new("automation.sqlite"),
        "SELECT CAST(COUNT(*) AS TEXT) AS value FROM wakeup_definitions",
    )
    .await?;
    assert_eq!(rows, ["0"]);
    Ok(())
}

pub async fn local_cancel_deliveries(
    stopped: &StoppedHost,
) -> FixtureResult<Vec<serde_json::Value>> {
    stopped_database_values(stopped, Path::new("interaction.sqlite"),
        "SELECT json_object('cancel',item_id,'target',target_item,'attribution',json(attribution_json)) AS value FROM local_cancel_deliveries ORDER BY item_id")
        .await?.iter().map(|row| serde_json::from_str(row).map_err(Into::into)).collect()
}

pub async fn held_local_sequence_stopped(
    stopped: &StoppedHost,
    item: ItemId,
) -> FixtureResult<fleet_protocol::ReportSeq> {
    let observations = super::messaging_stopped_owner::delivery_rows(stopped).await?;
    let header = super::messaging_stopped_owner::header(
        &observations,
        "inbox",
        &item.as_uuid().to_string(),
    )?;
    assert_eq!(header.get("route"), Some(&serde_json::json!("local")));
    assert_eq!(
        header.pointer("/progress/kind"),
        Some(&serde_json::json!("held"))
    );
    Ok(serde_json::from_value(
        header
            .get("reportSeq")
            .cloned()
            .ok_or("held outcome sequence absent")?,
    )?)
}

pub async fn prevented_cancel_stopped(
    stopped: &StoppedHost,
    item: ItemId,
    held_sequence: fleet_protocol::ReportSeq,
) -> FixtureResult<ItemId> {
    let observations = super::messaging_stopped_owner::delivery_rows(stopped).await?;
    let originals = observations
        .iter()
        .filter(|row| row.category == "inbox" && row.identity == item.as_uuid().to_string())
        .collect::<Vec<_>>();
    let [original] = originals.as_slice() else {
        return Err("original has no unique native Final outcome".into());
    };
    let header: serde_json::Value = serde_json::from_str(
        original
            .content
            .as_deref()
            .ok_or("original Inbox header absent")?,
    )?;
    assert_eq!(header.get("route"), Some(&serde_json::json!("relayed")));
    let cancel: ItemId = serde_json::from_value(
        header
            .pointer("/progress/value/outcome/value/cancel")
            .cloned()
            .ok_or("prevented cancel identity absent")?,
    )?;
    let cancel_header = super::messaging_stopped_owner::header(
        &observations,
        "inbox",
        &cancel.as_uuid().to_string(),
    )?;
    assert_eq!(
        cancel_header.get("cancelTarget"),
        Some(&serde_json::json!(item))
    );
    let prevented_sequence = held_sequence
        .next()
        .and_then(|sequence| sequence.next())
        .ok_or("local promotion report sequence exhausted")?;
    assert_eq!(
        header.get("reportSeq"),
        Some(&serde_json::json!(prevented_sequence))
    );
    assert_eq!(
        header.pointer("/progress/kind"),
        Some(&serde_json::json!("final"))
    );
    assert_eq!(
        header.pointer("/progress/value/outcome"),
        Some(&serde_json::json!({"kind":"prevented","value":{"cancel":cancel}}))
    );
    assert_refused_promotion_evidence(stopped, item, cancel, prevented_sequence, &observations)
        .await?;
    Ok(cancel)
}

// The local producer's frozen control supplies the expected identity before promotion.
pub async fn local_prevention_cancel(stopped: &StoppedHost, item: ItemId) -> FixtureResult<ItemId> {
    let records = local_cancel_deliveries(stopped).await?;
    let matching = records
        .iter()
        .filter(|record| record.get("target") == Some(&serde_json::json!(item)))
        .collect::<Vec<_>>();
    let [record] = matching.as_slice() else {
        return Err("local prevention has no unique frozen control".into());
    };
    Ok(serde_json::from_value(
        record
            .get("cancel")
            .cloned()
            .ok_or("local prevention control absent")?,
    )?)
}

pub async fn assert_single_cancelled_sender(stopped: &StoppedHost, item: ItemId) -> FixtureResult {
    let rows = super::messaging_stopped_owner::delivery_rows(stopped).await?;
    let original_count = rows
        .iter()
        .filter(|row| row.category == "outbox" && row.identity == item.as_uuid().to_string())
        .count();
    assert_eq!(
        original_count, 1,
        "takeover duplicated the sender's canonical item"
    );
    let header =
        super::messaging_stopped_owner::header(&rows, "outbox", &item.as_uuid().to_string())?;
    assert_eq!(
        header.pointer("/value/status/kind"),
        Some(&serde_json::json!("cancelled"))
    );
    Ok(())
}

async fn assert_refused_promotion_evidence(
    stopped: &StoppedHost,
    item: ItemId,
    cancel: ItemId,
    report_seq: fleet_protocol::ReportSeq,
    observations: &[fleet_mail_delivery::test_support::DeliveryObservation],
) -> FixtureResult {
    let reports = observations
        .iter()
        .filter(|row| row.category == "report" && row.identity == item.as_uuid().to_string())
        .collect::<Vec<_>>();
    let [retained] = reports.as_slice() else {
        return Err("refused promotion must retain exactly one report as evidence".into());
    };
    let report: serde_json::Value = serde_json::from_str(
        retained
            .content
            .as_deref()
            .ok_or("retained report absent")?,
    )?;
    assert_eq!(
        report,
        serde_json::json!({"item":item,"reportSeq":report_seq,"outcome":{"kind":"prevented","value":{"cancel":cancel}}})
    );
    // ReportOutcome::KEEP_RESULT=false removes the terminal journal operation;
    // its refused report evidence above remains until custody retention.
    let operations=stopped_database_values(stopped,Path::new("interaction.sqlite"),
        "SELECT request_key AS value FROM _fleet_sync_operations WHERE kind='fleet-sync.delivery.report-outcome'").await?;
    for request in operations {
        let (actual, _sequence): (ItemId, fleet_protocol::ReportSeq) =
            serde_json::from_str(&request)?;
        assert_ne!(
            actual, item,
            "terminal refusal left report operation or retry work"
        );
    }
    Ok(())
}
