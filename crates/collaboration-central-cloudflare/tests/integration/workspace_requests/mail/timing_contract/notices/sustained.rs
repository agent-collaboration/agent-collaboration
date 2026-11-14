//! R10-c: continued activity crosses every cadence repeatedly on one central alarm.
use super::*;

pub(super) async fn exercise_continued_activity(
    f: &Fixture,
    threads: &[(m::WorkspaceId, m::ThreadId)],
) {
    let mut recipients = Vec::new();
    for (index, (cadence, minutes)) in [
        (m::NoticeCadence::M15, 15_i64),
        (m::NoticeCadence::M25, 25),
        (m::NoticeCadence::M55, 55),
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
        recipients.push((address, minutes));
    }
    let r = f.host.central(&f.caller.organization_id).await;
    let rows=r.statement_host().read(Statement{sql:"SELECT actor_identity_id FROM actor_identities WHERE identity LIKE '%sustained-cadence-%' ORDER BY actor_identity_id",args:vec![]}).await.unwrap();
    let addressees: Vec<_> = rows
        .rows
        .iter()
        .map(|row| {
            let central_storage::SqlValue::Text(id) = &row[0] else {
                panic!("actor id")
            };
            m::Addressee::Actor {
                actor_identity_id: id.parse().unwrap(),
            }
        })
        .collect();
    assert_eq!(addressees.len(), 3);
    drop(r);
    let start = chrono::DateTime::from_timestamp((now(f).timestamp().div_euclid(300) + 1) * 300, 0)
        .unwrap();
    advance_to(f, start);
    let sources: Vec<_> = threads
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 2)
        .map(|(_, source)| source)
        .collect();
    let mut posts = Vec::new();
    for minute in 0..=110_i64 {
        let at = start + chrono::Duration::minutes(minute);
        let r = f.host.central(&f.caller.organization_id).await;
        let alarm = r
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap();
        drop(r);
        if alarm.is_some_and(|alarm| alarm <= at.timestamp_millis()) {
            dispatch_to(f, at).await;
        } else {
            advance_to(f, at);
        }
        if minute < 110 {
            let (workspace, thread) = sources[usize::try_from(minute).unwrap() % sources.len()];
            let item = created(
                call(
                    &f.host,
                    &f.caller,
                    Some(workspace.clone()),
                    44000 + u32::try_from(minute).unwrap(),
                    m::WorkspaceOperation::ThreadPost {
                        thread_id: thread.clone(),
                        body: format!("continued activity {minute}").try_into().unwrap(),
                        addressees: addressees.clone(),
                    },
                )
                .await,
            );
            posts.push((minute, workspace.clone(), thread.clone(), item));
        }
        let r = f.host.central(&f.caller.organization_id).await;
        for (address, cadence_minutes) in &recipients {
            let batch = r.due_for_session(address, at).await.unwrap();
            let expected_count = usize::try_from(minute / cadence_minutes).unwrap();
            assert_eq!(
                batch.messages.len(),
                expected_count,
                "cadence {cadence_minutes},minute {minute}"
            );
            let mut observed_posts = Vec::new();
            for (index, message) in batch.messages.iter().enumerate() {
                let due = start
                    + chrono::Duration::minutes(
                        (i64::try_from(index).unwrap() + 1) * cadence_minutes,
                    );
                assert_eq!(message.deliver_after, due);
                assert_eq!(due.timestamp().rem_euclid(300), 0);
                let MessageBody::Notice { items } = &message.body else {
                    panic!("continued activity notice")
                };
                assert!(!items.is_empty());
                for item in items {
                    observed_posts.push((
                        item.workspace_id.clone(),
                        item.thread_id.clone(),
                        item.item.clone(),
                    ));
                }
            }
            let through_minute = (minute / cadence_minutes) * cadence_minutes;
            let mut expected: Vec<_> = posts
                .iter()
                .filter(|(posted, _, _, _)| *posted < through_minute)
                .map(|(_, workspace, thread, item)| {
                    (workspace.clone(), Some(thread.clone()), item.clone())
                })
                .collect();
            observed_posts.sort_by_key(|entry| serde_json::to_string(entry).unwrap());
            expected.sort_by_key(|entry| serde_json::to_string(entry).unwrap());
            assert_eq!(
                observed_posts, expected,
                "exact continued activity at cadence {cadence_minutes},minute {minute}"
            );
        }
    }
}
