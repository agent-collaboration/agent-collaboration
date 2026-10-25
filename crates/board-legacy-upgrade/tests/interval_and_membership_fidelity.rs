#![allow(clippy::panic_in_result_fn)]
mod support;
use message_board::*;
use support::*;

#[tokio::test]
async fn joins_and_handover_keep_each_continuous_role_interval_without_posts() -> TestResult {
    let prepared = prepare("role-history.sqlite").await?;
    let project = prepared
        .projects
        .iter()
        .find(|project| project.store.project_id().as_str().ends_with("0015"))
        .ok_or("project absent")?;
    let thread = ThreadId::try_from("019a0000-0000-7000-8000-000000000065".to_owned())?;
    let mut snapshot = project.store.snapshot().await?;
    let keys = snapshot.participant_page_keys(&thread, None, 100).await?;
    let mut batch = snapshot.participant_interval_batch(&thread, &keys).await?;
    let mut actor_b = Vec::new();
    for key in keys {
        let interval = batch.take(&key)?;
        if matches!(&interval.identity,Identity::Session {session} if session.session_id.as_str()=="actor-b")
        {
            actor_b.push(interval);
        }
    }
    actor_b.sort_by_key(|row| row.joined_at_position.position);
    assert_eq!(
        actor_b.iter().map(|row| row.role).collect::<Vec<_>>(),
        vec![
            RecordedParticipantRole::Recorded(ParticipantRole::Advisor),
            RecordedParticipantRole::Recorded(ParticipantRole::Reviewer),
            RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
        ]
    );
    let reasons = actor_b
        .iter()
        .map(|row| {
            row.closed_reason
                .map(|reason| serde_json::to_string(&reason))
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        reasons,
        vec![
            Some("\"rejoined\"".into()),
            Some("\"promoted\"".into()),
            None
        ]
    );
    for adjacent in actor_b.windows(2) {
        if let [before, after] = adjacent {
            assert_eq!(before.closed_at_position, Some(after.joined_at_position));
        }
    }
    let human_post = PostId::try_from("019a0000-0000-7000-8000-000000000066".to_owned())?;
    let Some(Record::Post(post)) = snapshot.record(&human_post.into()).await? else {
        return Err("human post missing".into());
    };
    assert_eq!(post.posted_as_role, None);
    snapshot.finish().await?;
    let empty = prepared
        .projects
        .iter()
        .find(|project| project.store.project_id().as_str().ends_with("0016"))
        .ok_or("empty project missing")?;
    let snapshot = empty.store.snapshot().await?;
    assert_eq!(snapshot.high_water().position.get(), 1);
    snapshot.finish().await?;
    prepared.close().await;
    Ok(())
}

#[tokio::test]
async fn draining_membership_ends_at_the_last_recorded_event_even_after_reopen() -> TestResult {
    for name in ["legacy-complete.sqlite", "legacy-reopened.sqlite"] {
        let (_directory, path) = fixture_copy(name)?;
        let prepared = prepare_path(&path).await?;
        let source = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let mut times = Vec::new();
        for query in [
            "SELECT renewed_at AS at FROM thread_subscriptions UNION ALL SELECT ended_at FROM thread_subscriptions WHERE ended_at IS NOT NULL",
            "SELECT opened_at AS at FROM subscription_windows UNION ALL SELECT last_arrival_at FROM subscription_windows UNION ALL SELECT held_since FROM subscription_windows WHERE held_since IS NOT NULL",
        ] {
            let mut statement = source.prepare(query)?;
            for value in statement.query_map([], |row| row.get::<_, String>("at"))? {
                times.push(value?.parse::<chrono::DateTime<chrono::Utc>>()?);
            }
        }
        let mut cooldowns = source.prepare("SELECT last_post_at_ms FROM actor_board_cooldowns")?;
        for value in cooldowns.query_map([], |row| row.get::<_, i64>("last_post_at_ms"))? {
            times.push(
                chrono::DateTime::from_timestamp_millis(value?).ok_or("invalid source time")?,
            );
        }
        let expected = times
            .into_iter()
            .max()
            .ok_or("source has no recorded time")?;
        let reader = Identity::Session {
            session: SessionRef {
                endpoint: SessionEndpointRef {
                    service_id: ServiceId::try_from(
                        "550e8400-e29b-41d4-a716-446655440000".to_owned(),
                    )?,
                    endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
                },
                session_id: SessionId::try_from("creator-b".to_owned())?,
            },
        };
        let project = prepared
            .projects
            .iter()
            .find(|project| project.store.project_id().as_str().ends_with("0017"))
            .ok_or("project missing")?;
        let mut snapshot = project.store.snapshot().await?;
        let membership = snapshot
            .membership(
                &reader,
                &SubscriptionScope::thread(ThreadId::try_from(
                    "019a0000-0000-7000-8000-000000000066".to_owned(),
                )?),
            )
            .await?
            .ok_or("membership missing")?;
        assert_eq!(
            membership
                .ended()
                .ok_or("draining membership was kept active")?
                .at,
            expected
        );
        assert_eq!(
            membership.ended().ok_or("ending missing")?.reason,
            EndReason::Resolved
        );
        assert_eq!(membership.generation().get(), 3);
        assert_eq!(membership.policy().timing().quiet_seconds(), 3);
        assert_eq!(
            membership.expires_at(),
            "2023-11-14T23:21:40Z".parse::<chrono::DateTime<chrono::Utc>>()?
        );
        snapshot.finish().await?;
        prepared.close().await;
    }
    Ok(())
}
