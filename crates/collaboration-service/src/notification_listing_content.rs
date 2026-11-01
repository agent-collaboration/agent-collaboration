//! Listing text and count are derived together from the exact reserved member set.
use collaboration_protocol::{
    MachineId, PushHeaderFacts, PushId, PushKind, PushOrigin, PushRecord, PushRecordDraft,
    RouterLink, RouterOriginRef, SessionRef,
};
use message_board::BoardError;

pub(crate) fn draft_for_members(
    machine: MachineId,
    target: &SessionRef,
    id: PushId,
    members: &[PushRecord],
    created_at: chrono::DateTime<chrono::Utc>,
) -> Result<PushRecordDraft, BoardError> {
    let max = collaboration_protocol::limits::NOTIFICATION_LISTING_MAX_ENTRIES;
    let mut body = String::new();
    for member in members.iter().take(max) {
        let label = match member.kind {
            PushKind::BoardNotification => "Board activity",
            PushKind::SubscriptionActivity => "Thread activity",
            PushKind::SubscriptionExpiry => "Subscription expired",
            _ => return Err(BoardError::board_unavailable()),
        };
        body.push_str(&format!(
            "- {label}: {}\n",
            RouterLink::new(machine.clone(), member.push_id.clone())
        ));
    }
    if members.len() > max {
        let after = members
            .get(max - 1)
            .ok_or_else(BoardError::board_unavailable)?;
        body.push_str(&format!("{} notifications total. Read the rest with notification_members: listingId={}, after={}, limit={max}.",members.len(),id.as_str(),after.push_id.as_str()));
    }
    Ok(PushRecordDraft {
        push_id: id.clone(),
        kind: PushKind::NotificationListing,
        origin: PushOrigin::Router(PushKind::NotificationListing),
        origin_router_ref: Some(
            RouterOriginRef::NotificationListing {
                target: target.clone(),
                listing_id: id,
            }
            .canonical_string()
            .map_err(|_| BoardError::board_unavailable())?,
        ),
        target: target.clone(),
        mode: None,
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::NotificationListing {
            item_count: u64::try_from(members.len())
                .map_err(|_| BoardError::board_unavailable())?,
        },
        body: Some(body),
        activity: None,
        created_at,
    })
}
