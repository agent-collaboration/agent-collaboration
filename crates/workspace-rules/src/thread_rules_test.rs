use super::{ThreadReads, change_thread};
use crate::fixtures::{context, id, member, thread};
use workspace_model::{
    MemberRole, MembershipState, NotificationSetting, PostText, RefusalReason, ThreadCommand,
    ThreadState,
};
#[test]
fn r4_creator_only_and_repeated_resolution() {
    let mut context = context();
    let mut thread = thread();
    let command = ThreadCommand::ResolveThread {
        thread_id: id(8),
        closing_post: None,
    };
    context.actor = id(99);
    assert_eq!(
        change_thread(
            &context,
            &command,
            ThreadReads {
                thread: Some(&thread),
                ..Default::default()
            }
        )
        .unwrap_err(),
        RefusalReason::NotCreator { creator: id(2) }
    );
    context.actor = id(2);
    let plan = change_thread(
        &context,
        &command,
        ThreadReads {
            thread: Some(&thread),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        plan.batch
            .statements
            .first()
            .unwrap()
            .statement
            .sql
            .contains("creator=? AND state='open'")
    );
    thread.state = ThreadState::Resolved;
    assert!(matches!(
        change_thread(
            &context,
            &command,
            ThreadReads {
                thread: Some(&thread),
                ..Default::default()
            }
        ),
        Err(RefusalReason::ThreadResolved { .. })
    ));
}
#[test]
fn r5_observer_participant_and_left_member_posting() {
    let context = context();
    let thread = thread();
    let mut member = member(context.actor.clone(), MemberRole::Observer);
    let command = ThreadCommand::ThreadPost {
        post_id: id(30),
        thread_id: id(8),
        body: PostText::try_from("message".to_owned()).unwrap(),
        addressees: vec![],
    };
    assert!(matches!(
        change_thread(
            &context,
            &command,
            ThreadReads {
                thread: Some(&thread),
                caller_member: Some(&member),
                ..Default::default()
            }
        ),
        Err(RefusalReason::NotParticipant { .. })
    ));
    member.role = MemberRole::Participant;
    let plan = change_thread(
        &context,
        &command,
        ThreadReads {
            thread: Some(&thread),
            caller_member: Some(&member),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        plan.batch
            .statements
            .first()
            .unwrap()
            .statement
            .sql
            .starts_with("INSERT INTO workspace_posts")
    );
    member.state = MembershipState::Left { at: context.now };
    assert!(matches!(
        change_thread(
            &context,
            &command,
            ThreadReads {
                thread: Some(&thread),
                caller_member: Some(&member),
                ..Default::default()
            }
        ),
        Err(RefusalReason::NotMember { .. })
    ));
}
#[test]
fn r32_only_creator_adds_or_promotes_and_self_join_selects_role() {
    let mut context = context();
    let thread = thread();
    let target = member(id(40), MemberRole::Observer);
    let command = ThreadCommand::AddThreadMember {
        thread_id: id(8),
        member_id: id(41),
        actor: id(40),
        role: MemberRole::Participant,
    };
    context.actor = id(99);
    for target_member in [None, Some(&target)] {
        assert!(
            matches!(change_thread(&context,&command,ThreadReads {thread:Some(&thread),target_member,..Default::default()}),Err(RefusalReason::NotCreator {creator}) if creator == id(2))
        );
    }
    let joined = change_thread(
        &context,
        &ThreadCommand::JoinThread {
            thread_id: id(8),
            member_id: id(50),
            role: MemberRole::Observer,
        },
        ThreadReads {
            thread: Some(&thread),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        joined
            .batch
            .statements
            .first()
            .unwrap()
            .statement
            .args
            .contains(&central_storage::SqlValue::Text("observer".to_owned()))
    );
    context.actor = id(2);
    let added = change_thread(
        &context,
        &command,
        ThreadReads {
            thread: Some(&thread),
            ..Default::default()
        },
    )
    .unwrap();
    let insertion = &added.batch.statements.first().unwrap().statement;
    assert_eq!(
        insertion.sql,
        "INSERT INTO workspace_thread_members (thread_member_id,thread_id,actor_identity_id,member_role,notifications,left_at,created_at,updated_at) VALUES (?,?,?,?,1,NULL,?,?)"
    );
    assert_eq!(
        insertion.args.iter().take(4).cloned().collect::<Vec<_>>(),
        vec![
            central_storage::SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000029".to_owned()),
            central_storage::SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000008".to_owned()),
            central_storage::SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000028".to_owned()),
            central_storage::SqlValue::Text("participant".to_owned())
        ]
    );
    let promoted = change_thread(
        &context,
        &command,
        ThreadReads {
            thread: Some(&thread),
            target_member: Some(&target),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        promoted
            .batch
            .statements
            .first()
            .unwrap()
            .statement
            .sql
            .starts_with("UPDATE workspace_thread_members SET member_role=")
    );
    let mutation = &promoted.batch.statements.first().unwrap().statement;
    assert_eq!(
        mutation.sql,
        "UPDATE workspace_thread_members SET member_role=?,updated_at=? WHERE thread_member_id=? AND left_at IS NULL"
    );
    assert_eq!(
        mutation.args,
        vec![
            central_storage::SqlValue::Text("participant".to_owned()),
            central_storage::SqlValue::Text("2026-11-13T12:00:00+00:00".to_owned()),
            central_storage::SqlValue::Text("01890f2e-7b4c-7cc0-98c4-000000000009".to_owned())
        ]
    );
}
#[test]
fn notifications_off_keeps_membership() {
    let context = context();
    let thread = thread();
    let member = member(context.actor.clone(), MemberRole::Participant);
    let plan = change_thread(
        &context,
        &ThreadCommand::SetNotifications {
            thread_id: id(8),
            setting: NotificationSetting::Off,
        },
        ThreadReads {
            thread: Some(&thread),
            caller_member: Some(&member),
            ..Default::default()
        },
    )
    .unwrap();
    let sql = plan.batch.statements.first().unwrap().statement.sql;
    assert!(sql.starts_with("UPDATE workspace_thread_members SET notifications="));
    assert!(!sql.contains("SET left_at"));
}

#[test]
fn r32_self_join_promotes_only_the_callers_observer_and_preserves_notifications() {
    let mut context = context();
    context.actor = id(99);
    let thread = thread();
    let mut observer = member(context.actor.clone(), MemberRole::Observer);
    observer.notifications = NotificationSetting::Off;
    let command = ThreadCommand::JoinThread {
        thread_id: id(8),
        member_id: id(60),
        role: MemberRole::Participant,
    };
    let plan = change_thread(
        &context,
        &command,
        ThreadReads {
            thread: Some(&thread),
            caller_member: Some(&observer),
            ..Default::default()
        },
    )
    .unwrap();
    let mutation = &plan.batch.statements[0].statement;
    assert_eq!(
        mutation.sql,
        "UPDATE workspace_thread_members SET member_role='participant',updated_at=? WHERE thread_member_id=? AND left_at IS NULL"
    );
    assert_eq!(
        mutation.args[1],
        central_storage::SqlValue::Text(observer.thread_member_id.to_string())
    );
    let fresh = change_thread(
        &context,
        &command,
        ThreadReads {
            thread: Some(&thread),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        fresh.batch.statements[0]
            .statement
            .args
            .contains(&central_storage::SqlValue::Text("participant".to_owned()))
    );
}

fn sql_of(plan: &crate::RulePlan) -> Vec<&'static str> {
    plan.batch
        .statements
        .iter()
        .map(|row| {
            let words: Vec<&'static str> = row.statement.sql.split(' ').collect();
            match words.first() {
                Some(&"INSERT") => words.get(2).copied().unwrap_or(""),
                _ => words.get(1).copied().unwrap_or(""),
            }
        })
        .collect()
}

#[test]
fn creating_a_thread_writes_the_thread_membership_and_first_post_as_one_batch() {
    let context = context();
    let area = crate::fixtures::area();
    let plan = change_thread(
        &context,
        &ThreadCommand::CreateThread {
            thread_id: id(8),
            member_id: id(9),
            area_id: id(4),
            title: crate::fixtures::name("Thread"),
            first_post: crate::fixtures::first_post(31, 32),
        },
        ThreadReads {
            area: Some(&area),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        sql_of(&plan),
        vec![
            "workspace_threads",
            "workspace_thread_members",
            "workspace_changes",
            "workspace_posts",
            "workspace_changes"
        ]
    );
    let statements = &plan.batch.statements;
    let post_change = &statements[4].statement.args;
    assert_eq!(
        post_change[1],
        central_storage::SqlValue::Text("posted".to_owned())
    );
    assert_eq!(
        post_change[2],
        central_storage::SqlValue::Text("post".to_owned())
    );
    let thread_id: workspace_model::ThreadId = id(8);
    assert_eq!(
        post_change[4],
        central_storage::SqlValue::Text(thread_id.to_string())
    );
    let post_change_id: workspace_model::ChangeId = id(32);
    assert_eq!(
        post_change[0],
        central_storage::SqlValue::Text(post_change_id.to_string())
    );
    assert!(matches!(
        plan.answer,
        workspace_model::RuleAnswer::Created {
            item: workspace_model::ItemRef::Thread(_)
        }
    ));
}

#[test]
fn an_unheld_role_addressee_refuses_the_whole_thread_creation() {
    let area = crate::fixtures::area();
    let mut first_post = crate::fixtures::first_post(31, 32);
    first_post.addressees = vec![workspace_model::Addressee::ProjectRole {
        project_id: id(5),
        role: workspace_model::ProjectRole::Advisor,
    }];
    let refused = change_thread(
        &context(),
        &ThreadCommand::CreateThread {
            thread_id: id(8),
            member_id: id(9),
            area_id: id(4),
            title: crate::fixtures::name("Thread"),
            first_post,
        },
        ThreadReads {
            area: Some(&area),
            ..Default::default()
        },
    );
    assert_eq!(refused.unwrap_err(), RefusalReason::UnheldAddressee);
}

#[test]
fn a_closing_post_is_written_before_the_thread_resolves_in_the_same_batch() {
    let context = context();
    let thread = thread();
    let creator = member(id(2), MemberRole::Participant);
    let command = ThreadCommand::ResolveThread {
        thread_id: id(8),
        closing_post: Some(crate::fixtures::first_post(31, 32)),
    };
    let plan = change_thread(
        &context,
        &command,
        ThreadReads {
            thread: Some(&thread),
            caller_member: Some(&creator),
            ..Default::default()
        },
    )
    .unwrap();
    let mut observer = member(id(2), MemberRole::Observer);
    let as_observer = change_thread(
        &context,
        &command,
        ThreadReads {
            thread: Some(&thread),
            caller_member: Some(&observer),
            ..Default::default()
        },
    );
    observer.state = MembershipState::Left {
        at: "2026-11-13T12:00:00Z".parse().unwrap(),
    };
    let after_leaving = change_thread(
        &context,
        &command,
        ThreadReads {
            thread: Some(&thread),
            caller_member: Some(&observer),
            ..Default::default()
        },
    );

    assert_eq!(
        sql_of(&plan),
        vec![
            "workspace_posts",
            "workspace_changes",
            "workspace_threads",
            "workspace_changes"
        ]
    );
    assert_eq!(
        plan.batch.statements[3].statement.args[1],
        central_storage::SqlValue::Text("resolved".to_owned())
    );
    assert!(matches!(
        as_observer,
        Err(RefusalReason::NotParticipant { .. })
    ));
    assert!(matches!(
        after_leaving,
        Err(RefusalReason::NotMember { .. })
    ));
}

/// A drawn first or closing post is admitted only as valid PostText, lands in
/// the same batch as its thread step, and the same command plans the same batch.
#[hegel::test]
fn thread_steps_with_posts_are_atomic_and_deterministic(case: hegel::TestCase) {
    use hegel::generators as gs;
    let raw: String = case.draw(gs::text().max_size(200));
    let closing = case.draw(gs::booleans());
    let Ok(body) = PostText::try_from(raw.clone()) else {
        assert!(raw.trim().is_empty() || raw.chars().any(char::is_control));
        return;
    };
    let draft = workspace_model::ThreadPostDraft {
        post_id: id(31),
        change_id: id(32),
        body,
        addressees: vec![],
    };
    let area = crate::fixtures::area();
    let thread = thread();
    let creator = member(id(2), MemberRole::Participant);
    let command = if closing {
        ThreadCommand::ResolveThread {
            thread_id: id(8),
            closing_post: Some(draft),
        }
    } else {
        ThreadCommand::CreateThread {
            thread_id: id(8),
            member_id: id(9),
            area_id: id(4),
            title: crate::fixtures::name("Thread"),
            first_post: draft,
        }
    };
    let reads = || ThreadReads {
        area: Some(&area),
        thread: Some(&thread),
        caller_member: Some(&creator),
        ..Default::default()
    };

    let first = change_thread(&context(), &command, reads()).unwrap();
    let second = change_thread(&context(), &command, reads()).unwrap();

    assert_eq!(first, second);
    let tables = sql_of(&first);
    assert_eq!(
        tables
            .iter()
            .filter(|table| **table == "workspace_posts")
            .count(),
        1
    );
    assert_eq!(
        tables
            .iter()
            .filter(|table| **table == "workspace_changes")
            .count(),
        2
    );
    let post_row = first
        .batch
        .statements
        .iter()
        .find(|row| row.statement.sql.starts_with("INSERT INTO workspace_posts"))
        .unwrap();
    assert_eq!(
        post_row.statement.args[3],
        central_storage::SqlValue::Text(raw)
    );
}
