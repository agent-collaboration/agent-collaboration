//! Resolves local push links, applies reported-caller access rules, and expands stored ranges.
use crate::ServiceIdentity;
use collaboration_protocol::{
    MachineId, PushActivityRangeRead, PushId, PushKind, PushLineInput, PushOrigin, PushRecord,
    PushRecordNotice, RouterLink, SessionRef, render_push_line,
};
use message_board::{FeedContent, FeedPosition, Placement, Record};
use message_board_storage::{FeedPageBounds, FeedPageDirection};

pub(crate) fn link_for(record: &PushRecord, identity: &ServiceIdentity) -> String {
    RouterLink::new(
        MachineId::from(identity.machine_identity.service_id().clone()),
        record.push_id.clone(),
    )
    .to_string()
}

pub(crate) fn line_for(
    record: &PushRecord,
    identity: &ServiceIdentity,
) -> Result<String, collaboration_protocol::PushLineError> {
    render_push_line(&PushLineInput {
        link: RouterLink::new(
            MachineId::from(identity.machine_identity.service_id().clone()),
            record.push_id.clone(),
        ),
        machine_label: identity.machine_identity.machine_label().clone(),
        origin: record.origin.clone(),
        header_facts: record.header_facts.clone(),
        body: record.body.clone(),
    })
}

pub(crate) fn resolve_reference(
    reference: &str,
    identity: &ServiceIdentity,
) -> Result<PushId, ReferenceError> {
    if let Ok(push_id) = PushId::try_from(reference.to_owned()) {
        return Ok(push_id);
    }
    let link = RouterLink::parse(reference).map_err(|_| ReferenceError::Invalid)?;
    let local_machine_id = String::from(identity.machine_identity.service_id().clone());
    if link.machine_id().as_str() != local_machine_id {
        return Err(ReferenceError::Foreign(
            link.machine_id().as_str().to_owned(),
        ));
    }
    Ok(link.push_id().clone())
}

pub(crate) fn can_read(record: &PushRecord, caller: &SessionRef) -> bool {
    if &record.target == caller {
        return true;
    }
    record.kind == PushKind::DirectMessage
        && matches!(&record.origin, PushOrigin::Session(origin) if origin == caller)
}

pub(crate) fn caller_is_local(caller: &SessionRef, identity: &ServiceIdentity) -> bool {
    caller.endpoint.service_id == identity.service_id
}

pub(crate) fn make_notice_list(
    records: Vec<PushRecord>,
    identity: &ServiceIdentity,
) -> Result<Vec<PushRecordNotice>, collaboration_protocol::PushLineError> {
    records
        .into_iter()
        .map(|record| {
            Ok(PushRecordNotice {
                push_id: record.push_id.clone(),
                link: link_for(&record, identity),
                line: line_for(&record, identity)?,
                origin: record.origin.clone(),
                target: record.target.clone(),
                reply_to_push_id: record.reply_to_push_id.clone(),
                delivery_state: record.delivery_state,
                cancelled_after_delivery: record.cancelled_after_delivery.clone(),
                created_at: record.created_at,
                read_at: record.read_at,
            })
        })
        .collect()
}

pub(crate) enum ActivityReadError {
    Unavailable,
    Failed,
}

pub(crate) async fn expand_activity_ranges(
    record: &PushRecord,
    identity: &ServiceIdentity,
) -> Result<Vec<PushActivityRangeRead>, ActivityReadError> {
    let Some(snapshot) = record.activity.as_ref() else {
        return Ok(Vec::new());
    };
    let Some(admission) = identity.project_admission.as_ref() else {
        return Err(ActivityReadError::Unavailable);
    };
    let mut ranges = Vec::with_capacity(snapshot.ranges.len());
    for range in &snapshot.ranges {
        let store = admission
            .registry
            .read_project(&range.project_id)
            .await
            .map_err(|_| ActivityReadError::Failed)?
            .ok_or(ActivityReadError::Unavailable)?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|_| ActivityReadError::Failed)?;
        if range.from.position > range.through.position
            || range.through.position > read.high_water().position
            || !matches!(
                read.cursor_validity(range.from)
                    .await
                    .map_err(|_| ActivityReadError::Failed)?,
                message_board_storage::CursorValidity::Valid
            )
            || !matches!(
                read.cursor_validity(range.through)
                    .await
                    .map_err(|_| ActivityReadError::Failed)?,
                message_board_storage::CursorValidity::Valid
            )
        {
            return Err(ActivityReadError::Failed);
        }
        let mut after = read
            .feed_cursor_or_start(
                message_board::FeedPosition::try_from(range.from.position.get().saturating_sub(1))
                    .map_err(|_| ActivityReadError::Failed)?,
            )
            .await
            .map_err(|_| ActivityReadError::Failed)?;
        let mut posts = Vec::new();
        while after.position < range.through.position {
            let limit = (range.through.position.get() - after.position.get()).min(100);
            let covered_through = read
                .feed_cursor_or_start(
                    FeedPosition::try_from(after.position.get() + limit)
                        .map_err(|_| ActivityReadError::Failed)?,
                )
                .await
                .map_err(|_| ActivityReadError::Failed)?;
            read.validate_feed_range(after.position, covered_through.position)
                .await
                .map_err(|_| ActivityReadError::Failed)?;
            let bounds = FeedPageBounds {
                after: after.position,
                through: covered_through.position,
                direction: FeedPageDirection::Forward,
                limit: limit + 1,
                sweep: None,
            };
            let mut keys = read
                .post_activity_keys(&bounds, Some(&range.thread_id))
                .await
                .map_err(|_| ActivityReadError::Failed)?;
            keys.retain(|key| matches!(&key.placement, Placement::Thread { thread_id } if thread_id == &range.thread_id));
            let feed_keys: Vec<_> = keys.iter().map(|key| key.feed.clone()).collect();
            let mut bodies = read
                .feed_entry_batch(&feed_keys)
                .await
                .map_err(|_| ActivityReadError::Failed)?;
            for key in keys {
                let entry = bodies
                    .remove(&key.feed.cursor.position)
                    .ok_or(ActivityReadError::Failed)?
                    .decode(false)
                    .map_err(|_| ActivityReadError::Failed)?;
                if let FeedContent::Shown(record) = entry.content
                    && let Record::Post(post) = *record
                {
                    if post.placement != key.placement || post.actor != key.actor {
                        return Err(ActivityReadError::Failed);
                    }
                    posts.push(post);
                }
            }
            after = covered_through;
        }
        read.finish().await.map_err(|_| ActivityReadError::Failed)?;
        ranges.push(PushActivityRangeRead {
            range: range.clone(),
            posts,
        });
    }
    Ok(ranges)
}

pub(crate) enum ReferenceError {
    Invalid,
    Foreign(String),
}
