use super::{InboxReads, ScopedChange, inbox_answer};
use crate::fixtures::{area, context, id, member, project, thread};
use hegel::{TestCase, generators as gs};
use workspace_model::{
    Change, ChangeEvent, ChangePosition, InboxItem, InboxReadMode, InboxScope, ItemRef, ListOrder,
    MemberRole, NotificationSetting, PageLimit, Post, PostText, ReadMark, ReadScope, RefusalReason,
    ThreadState, WorkspaceAnswer,
};

fn position(value: u64) -> ChangePosition {
    value.try_into().unwrap()
}
fn change(at: u64, item: ItemRef, event: ChangeEvent) -> Change {
    Change {
        change_id: id(u32::try_from(1000 + at).unwrap()),
        position: position(at),
        item,
        actor: id(2),
        event,
    }
}
fn in_thread(at: u64, item: ItemRef, event: ChangeEvent) -> ScopedChange {
    ScopedChange {
        change: change(at, item, event),
        thread_id: Some(id(8)),
        project_id: None,
    }
}
fn in_project(at: u64, item: ItemRef, event: ChangeEvent) -> ScopedChange {
    ScopedChange {
        change: change(at, item, event),
        thread_id: None,
        project_id: Some(id(5)),
    }
}
fn post(value: u32) -> Post {
    Post {
        post_id: id(value),
        thread_id: id(8),
        author: id(2),
        body: PostText::try_from("hello".to_owned()).unwrap(),
        addressees: vec![],
    }
}
struct Page {
    order: ListOrder,
    positions: Vec<u64>,
    items: Vec<InboxItem>,
    next: Option<ChangePosition>,
}
fn read(
    scope: &InboxScope,
    mode: InboxReadMode,
    after: Option<ChangePosition>,
    limit: u16,
    reads: &InboxReads<'_>,
) -> Page {
    let WorkspaceAnswer::Inbox {
        order,
        items,
        next_after_position,
        unavailable_projects,
        ..
    } = inbox_answer(
        scope,
        mode,
        after,
        PageLimit::try_from(limit).unwrap(),
        reads,
    )
    .unwrap()
    else {
        panic!("inbox answer");
    };
    assert!(unavailable_projects.is_empty());
    Page {
        order,
        positions: items.iter().map(|item| item.position().get()).collect(),
        items,
        next: next_after_position,
    }
}

#[test]
fn an_area_inbox_shows_followed_posts_and_open_and_resolved_states_only() {
    let reader = context().actor;
    let changes = [
        in_thread(1, ItemRef::Thread(id(8)), ChangeEvent::Created),
        in_thread(2, ItemRef::Post(id(30)), ChangeEvent::Posted),
        in_thread(
            3,
            ItemRef::Thread(id(8)),
            ChangeEvent::Joined { member: id(9) },
        ),
        in_thread(4, ItemRef::Thread(id(8)), ChangeEvent::Resolved),
        in_project(5, ItemRef::Project(id(5)), ChangeEvent::StateChanged),
    ];
    let reads = InboxReads {
        reader: &reader,
        areas: &[area()],
        projects: &[project()],
        threads: &[thread()],
        memberships: &[],
        posts: &[post(30)],
        changes: &changes,
        marks: &[],
        high_water: position(5),
    };
    let scope = InboxScope::Area { area_id: id(4) };

    let page = read(&scope, InboxReadMode::Unread, None, 10, &reads);

    assert_eq!(page.order, ListOrder::OldestFirst);
    assert_eq!(page.positions, vec![1, 2, 4]);
    let acknowledge = ReadScope::InboxArea { area_id: id(4) };
    assert!(
        matches!(&page.items[0], InboxItem::ThreadStateChanged { state: ThreadState::Open, acknowledge: a, .. } if *a == acknowledge)
    );
    assert!(
        matches!(&page.items[1], InboxItem::PostCreated { post, thread_title, area_name, project_name: None, .. }
        if post.post_id == id(30) && thread_title.as_str() == "Thread" && area_name.as_str() == "Area")
    );
    assert!(matches!(
        &page.items[2],
        InboxItem::ThreadStateChanged {
            state: ThreadState::Resolved,
            ..
        }
    ));
}

#[test]
fn unfollowed_threads_stay_out_of_an_area_inbox() {
    let reader = id(99);
    let changes = [in_thread(1, ItemRef::Post(id(30)), ChangeEvent::Posted)];
    let mut muted = member(id(99), MemberRole::Participant);
    muted.notifications = NotificationSetting::Off;
    let with = |memberships: &[workspace_model::ThreadMember]| {
        let reads = InboxReads {
            reader: &reader,
            areas: &[area()],
            projects: &[],
            threads: &[thread()],
            memberships,
            posts: &[post(30)],
            changes: &changes,
            marks: &[],
            high_water: position(1),
        };
        read(
            &InboxScope::Area { area_id: id(4) },
            InboxReadMode::Unread,
            None,
            10,
            &reads,
        )
        .positions
    };
    assert_eq!(with(&[]), Vec::<u64>::new());
    assert_eq!(with(&[muted]), Vec::<u64>::new());
    assert_eq!(with(&[member(id(99), MemberRole::Observer)]), vec![1]);
}

#[test]
fn a_project_inbox_carries_the_projects_changes_with_its_names() {
    let reader = context().actor;
    let changes = [
        in_thread(1, ItemRef::Post(id(30)), ChangeEvent::Posted),
        in_project(2, ItemRef::Project(id(5)), ChangeEvent::StateChanged),
        ScopedChange {
            project_id: Some(id(50)),
            ..in_project(3, ItemRef::Project(id(50)), ChangeEvent::StateChanged)
        },
    ];
    let reads = InboxReads {
        reader: &reader,
        areas: &[area()],
        projects: &[project()],
        threads: &[thread()],
        memberships: &[],
        posts: &[post(30)],
        changes: &changes,
        marks: &[],
        high_water: position(3),
    };

    let page = read(
        &InboxScope::Project { project_id: id(5) },
        InboxReadMode::Unread,
        None,
        10,
        &reads,
    );

    assert_eq!(page.positions, vec![2]);
    assert!(
        matches!(&page.items[0], InboxItem::ProjectChanged { acknowledge: ReadScope::InboxProject { project_id }, project_name, area_name, change, .. }
        if *project_id == id(5) && project_name.as_str() == "Project X" && area_name.as_str() == "Area" && change.event == ChangeEvent::StateChanged)
    );
}

#[test]
fn unread_starts_after_the_scope_mark_and_latest_ignores_it() {
    let reader = context().actor;
    let changes: Vec<_> = (1..=5)
        .map(|at| in_project(at, ItemRef::Project(id(5)), ChangeEvent::Updated))
        .collect();
    let marks = [
        // Another scope's later mark comes first and must not be read.
        ReadMark {
            read_mark_id: id(11),
            actor_identity_id: reader.clone(),
            scope: ReadScope::InboxArea { area_id: id(4) },
            through_position: position(5),
        },
        ReadMark {
            read_mark_id: id(10),
            actor_identity_id: reader.clone(),
            scope: ReadScope::InboxProject { project_id: id(5) },
            through_position: position(3),
        },
    ];
    let reads = InboxReads {
        reader: &reader,
        areas: &[area()],
        projects: &[project()],
        threads: &[],
        memberships: &[],
        posts: &[],
        changes: &changes,
        marks: &marks,
        high_water: position(5),
    };
    let scope = InboxScope::Project { project_id: id(5) };

    let unread = read(&scope, InboxReadMode::Unread, None, 10, &reads);
    let latest = read(&scope, InboxReadMode::Latest, None, 2, &reads);
    let older = read(&scope, InboxReadMode::Latest, latest.next, 2, &reads);

    assert_eq!(unread.positions, vec![4, 5]);
    assert_eq!(latest.order, ListOrder::NewestFirst);
    assert_eq!(latest.positions, vec![5, 4]);
    assert_eq!(latest.next, Some(position(4)));
    assert_eq!(older.positions, vec![3, 2]);
}

#[test]
fn an_unknown_scope_is_not_found() {
    let reader = context().actor;
    let reads = InboxReads {
        reader: &reader,
        areas: &[],
        projects: &[],
        threads: &[],
        memberships: &[],
        posts: &[],
        changes: &[],
        marks: &[],
        high_water: ChangePosition::ZERO,
    };
    assert!(matches!(
        inbox_answer(
            &InboxScope::Area { area_id: id(4) },
            InboxReadMode::Unread,
            None,
            PageLimit::try_from(1).unwrap(),
            &reads
        ),
        Err(RefusalReason::NotFound { .. })
    ));
}

/// Unread pages walk every project change after the mark exactly once, oldest
/// first, and the same read answers the same.
#[hegel::test]
fn unread_paging_walks_every_change_after_the_mark_once(case: TestCase) {
    let reader = context().actor;
    let count = case.draw(gs::integers::<u64>().min_value(0).max_value(25));
    let mark = case.draw(gs::integers::<u64>().min_value(0).max_value(count));
    let size = case.draw(gs::integers::<u16>().min_value(1).max_value(6));
    let changes: Vec<_> = (1..=count)
        .rev()
        .map(|at| in_project(at, ItemRef::Project(id(5)), ChangeEvent::Updated))
        .collect();
    let marks = [ReadMark {
        read_mark_id: id(10),
        actor_identity_id: reader.clone(),
        scope: ReadScope::InboxProject { project_id: id(5) },
        through_position: position(mark),
    }];
    let reads = InboxReads {
        reader: &reader,
        areas: &[area()],
        projects: &[project()],
        threads: &[],
        memberships: &[],
        posts: &[],
        changes: &changes,
        marks: &marks,
        high_water: position(count),
    };
    let scope = InboxScope::Project { project_id: id(5) };

    let mut walked = Vec::new();
    let mut after = None;
    loop {
        let page = read(&scope, InboxReadMode::Unread, after, size, &reads);
        let again = read(&scope, InboxReadMode::Unread, after, size, &reads);
        assert_eq!(page.positions, again.positions);
        assert!(page.positions.len() <= usize::from(size));
        walked.extend(page.positions);
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }

    assert_eq!(walked, (mark + 1..=count).collect::<Vec<_>>());
}
