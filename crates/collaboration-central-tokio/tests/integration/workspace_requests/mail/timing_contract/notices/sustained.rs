//! R10-c: different activity times share epoch slots across repeated hour boundaries.
use super::*;

pub(super) async fn exercise_continued_activity(
    f: &Fixture,
    threads: &[(m::WorkspaceId, m::ThreadId)],
) {
    advance_to(f, "2030-01-01T02:01:00Z".parse().unwrap());
    let mut recipients = Vec::new();
    for (index, cadence) in [
        m::NoticeCadence::M15,
        m::NoticeCadence::M25,
        m::NoticeCadence::M55,
        m::NoticeCadence::M25,
    ]
    .into_iter()
    .enumerate()
    {
        let mut caller = f.caller.clone();
        let mut address = session(&caller);
        address.session_id = format!("sustained-cadence-{index}").try_into().unwrap();
        caller.caller = m::Caller::Session(address.clone());
        assert!(matches!(
            request(
                f,
                &caller,
                43000 + u32::try_from(index).unwrap(),
                Op::SetNoticeCadence { cadence }
            )
            .await,
            Result::Done(_)
        ));
        recipients.push(address);
    }
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let rows = r.statement_host().read(Statement {sql:"SELECT identity,actor_identity_id FROM actor_identities WHERE identity LIKE '%sustained-cadence-%'",args:vec![]}).await.unwrap();
    let addressees: Vec<_> = recipients
        .iter()
        .map(|address| {
            let row = rows
                .rows
                .iter()
                .find(|row| {
                    let central_storage::SqlValue::Text(identity) = &row[0] else {
                        panic!("identity")
                    };
                    let m::Identity::AgentSession { session } =
                        serde_json::from_str(identity).unwrap()
                    else {
                        panic!("session")
                    };
                    &session == address
                })
                .unwrap();
            let central_storage::SqlValue::Text(actor) = &row[1] else {
                panic!("actor")
            };
            m::Addressee::Actor {
                actor_identity_id: actor.parse().unwrap(),
            }
        })
        .collect();
    assert_eq!(addressees.len(), 4);
    drop(r);
    // Literal slot offsets and UTC marks are independent of production's mapping.
    let slots: [&[(i64, &str)]; 4] = [
        &[
            (14, "02:15"),
            (29, "02:30"),
            (44, "02:45"),
            (59, "03:00"),
            (74, "03:15"),
            (89, "03:30"),
            (104, "03:45"),
        ],
        &[
            (4, "02:05"),
            (29, "02:30"),
            (54, "02:55"),
            (79, "03:20"),
            (104, "03:45"),
        ],
        &[(14, "02:15"), (69, "03:10")],
        &[
            (4, "02:05"),
            (29, "02:30"),
            (54, "02:55"),
            (79, "03:20"),
            (104, "03:45"),
        ],
    ];
    let start: m::Timestamp = "2030-01-01T02:01:00Z".parse().unwrap();
    let sources: Vec<_> = threads
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 2)
        .map(|(_, source)| source)
        .collect();
    let mut posts = Vec::new();
    for minute in 0..=110_i64 {
        let at = start + chrono::Duration::minutes(minute);
        let r = f.host.central(&f.caller.organization_id).await.unwrap();
        let alarm = r.statement_host().alarm().map(|due| due.unix_millis());
        drop(r);
        if alarm.is_some_and(|alarm| alarm <= at.timestamp_millis()) {
            dispatch_to(f, at).await;
        } else {
            advance_to(f, at);
        }
        if minute < 110 {
            let (workspace, thread) = sources[usize::try_from(minute).unwrap() % sources.len()];
            let selected = [0, if minute % 2 == 0 { 1 } else { 3 }, 2];
            let item = created(
                call(
                    &f.host,
                    &f.caller,
                    Some(workspace.clone()),
                    44000 + u32::try_from(minute).unwrap(),
                    m::WorkspaceOperation::ThreadPost {
                        thread_id: thread.clone(),
                        body: format!("continued activity {minute}").try_into().unwrap(),
                        addressees: selected
                            .iter()
                            .map(|index| addressees[*index].clone())
                            .collect(),
                    },
                )
                .await,
            );
            posts.push((minute, workspace.clone(), thread.clone(), item, selected));
        }
        let r = f.host.central(&f.caller.organization_id).await.unwrap();
        for (recipient_index, address) in recipients.iter().enumerate() {
            let queue = r.queued_for_session(address).await.unwrap();
            let elapsed: Vec<_> = slots[recipient_index]
                .iter()
                .filter(|(offset, _)| *offset <= minute)
                .collect();
            assert_eq!(
                queue.messages.len(),
                elapsed.len(),
                "recipient {recipient_index}, minute {minute}"
            );
            let mut observed = Vec::new();
            let mut ids = std::collections::BTreeSet::new();
            for (message, (_, literal)) in queue.messages.iter().zip(&elapsed) {
                let due: m::Timestamp = format!("2030-01-01T{literal}:00Z").parse().unwrap();
                assert_eq!(
                    message.created_at, due,
                    "recipient {recipient_index}, minute {minute}"
                );
                assert!(ids.insert(message.message_id.clone()));
                assert_eq!(
                    message.state,
                    workspace_model::central_mail::MessageState::Queued
                );
                let MessageBody::Notice { items } = &message.body else {
                    panic!("notice")
                };
                assert!(!items.is_empty());
                observed.extend(items.iter().map(|item| {
                    (
                        item.workspace_id.clone(),
                        item.thread_id.clone(),
                        item.item.clone(),
                    )
                }));
            }
            let through = elapsed.last().map_or(0, |(offset, _)| *offset);
            let mut expected: Vec<_> = posts
                .iter()
                .filter(|(posted, _, _, _, selected)| {
                    *posted < through && selected.contains(&recipient_index)
                })
                .map(|(_, workspace, thread, item, _)| {
                    (workspace.clone(), Some(thread.clone()), item.clone())
                })
                .collect();
            observed.sort_by_key(|item| serde_json::to_string(item).unwrap());
            expected.sort_by_key(|item| serde_json::to_string(item).unwrap());
            assert_eq!(
                observed, expected,
                "exact recipient {recipient_index}, minute {minute}"
            );
        }
    }
}
