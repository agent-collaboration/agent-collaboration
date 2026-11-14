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
    let command = ThreadCommand::ResolveThread { thread_id: id(8) };
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
            .starts_with("INSERT INTO posts")
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
        "INSERT INTO thread_members (thread_member_id,thread_id,actor_identity_id,member_role,notifications,left_at,created_at,updated_at) VALUES (?,?,?,?,1,NULL,?,?)"
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
            .starts_with("UPDATE thread_members SET member_role=")
    );
    let mutation = &promoted.batch.statements.first().unwrap().statement;
    assert_eq!(
        mutation.sql,
        "UPDATE thread_members SET member_role=?,updated_at=? WHERE thread_member_id=? AND left_at IS NULL"
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
    assert!(sql.starts_with("UPDATE thread_members SET notifications="));
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
        "UPDATE thread_members SET member_role='participant',updated_at=? WHERE thread_member_id=? AND left_at IS NULL"
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
