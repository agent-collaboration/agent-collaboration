use super::{ListReads, cursor_page, list_answer};
use crate::fixtures::{area, context, deliverable, id, issue, member, name, project, thread};
use crate::read_rules::PositionedItem;
use hegel::{TestCase, generators as gs};
use workspace_model::{
    ActorIdentityId, AreaState, AssignmentState, ExternalLink, ItemRef, ItemView, ListCursor,
    ListOrder, ListQuery, MemberRole, MembershipState, PageLimit, Post, PostScope, PostText,
    ProjectRole, ProjectRoleAssignment, Reference, ReferenceSource, RefusalReason, SearchQuery,
    SearchableKind, Thread, ThreadMember, TrailTag, WorkLogEntry, WorkStatus, WorkspaceAnswer,
};

fn at(position: u64, item: ItemView) -> PositionedItem<ItemView> {
    PositionedItem {
        position: position.try_into().unwrap(),
        item,
    }
}
fn limit(value: u16) -> PageLimit {
    PageLimit::try_from(value).unwrap()
}
fn search(text: &str) -> SearchQuery {
    SearchQuery::try_from(text.to_owned()).unwrap()
}
fn post(value: u32, thread_id: u32, body: &str) -> ItemView {
    ItemView::Post(Post {
        post_id: id(value),
        thread_id: id(thread_id),
        author: id(2),
        body: PostText::try_from(body.to_owned()).unwrap(),
        addressees: vec![],
    })
}
fn thread_view(thread: Thread) -> ItemView {
    ItemView::Thread { thread, me: None }
}
fn page(answer: WorkspaceAnswer) -> (Vec<ItemRef>, Option<ListCursor>) {
    match answer {
        WorkspaceAnswer::Page { items, next_cursor } => {
            (items.iter().map(ItemView::item_ref).collect(), next_cursor)
        }
        WorkspaceAnswer::References { items, next_cursor } => (
            items
                .into_iter()
                .map(|row| ItemRef::Reference(row.reference_id))
                .collect(),
            next_cursor,
        ),
        WorkspaceAnswer::Links { items, next_cursor } => (
            items
                .into_iter()
                .map(|row| ItemRef::Link(row.link_id))
                .collect(),
            next_cursor,
        ),
        other => panic!("not a list answer: {other:?}"),
    }
}
struct Fixture {
    reader: ActorIdentityId,
    items: Vec<PositionedItem<ItemView>>,
    threads: Vec<Thread>,
    members: Vec<ThreadMember>,
}
impl Fixture {
    fn new(items: Vec<PositionedItem<ItemView>>) -> Self {
        let threads = items
            .iter()
            .filter_map(|row| match &row.item {
                ItemView::Thread { thread, .. } => Some(thread.clone()),
                _ => None,
            })
            .collect();
        let members = items
            .iter()
            .filter_map(|row| match &row.item {
                ItemView::ThreadMember(member) => Some(member.clone()),
                _ => None,
            })
            .collect();
        Self {
            reader: context().actor,
            items,
            threads,
            members,
        }
    }
    fn list(
        &self,
        query: ListQuery,
        cursor: ListCursor,
        size: u16,
    ) -> (Vec<ItemRef>, Option<ListCursor>) {
        page(
            list_answer(
                &query,
                cursor,
                limit(size),
                &ListReads {
                    reader: &self.reader,
                    items: &self.items,
                    threads: &self.threads,
                    memberships: &self.members,
                },
            )
            .unwrap(),
        )
    }
    fn all(&self, query: ListQuery) -> Vec<ItemRef> {
        self.list(query, ListCursor::start(ListOrder::OldestFirst), 100)
            .0
    }
}

#[test]
fn r8_order_comes_from_positions_not_input_or_ids() {
    let rows = [
        PositionedItem {
            position: 3.try_into().unwrap(),
            item: "third",
        },
        PositionedItem {
            position: 1.try_into().unwrap(),
            item: "first",
        },
        PositionedItem {
            position: 2.try_into().unwrap(),
            item: "redacted",
        },
    ];
    let oldest = cursor_page(&rows, ListCursor::start(ListOrder::OldestFirst), limit(10));
    let newest = cursor_page(&rows, ListCursor::start(ListOrder::NewestFirst), limit(10));
    assert_eq!(oldest.items, vec!["first", "redacted", "third"]);
    assert_eq!(newest.items, vec!["third", "redacted", "first"]);
    assert_eq!(oldest.next_cursor, None);
}

#[test]
fn newest_first_continues_strictly_before_the_last_position() {
    let rows: Vec<_> = (1..=5)
        .map(|value| PositionedItem {
            position: value.try_into().unwrap(),
            item: value,
        })
        .collect();
    let first = cursor_page(&rows, ListCursor::start(ListOrder::NewestFirst), limit(2));
    let next = first.next_cursor.unwrap();
    let second = cursor_page(&rows, next, limit(2));
    assert_eq!(first.items, vec![5, 4]);
    assert_eq!(next.order(), ListOrder::NewestFirst);
    assert_eq!(next.position(), Some(4.try_into().unwrap()));
    assert_eq!(second.items, vec![3, 2]);
}

#[test]
fn relation_pages_use_own_created_positions_not_source_or_id_order() {
    let reference = |value| {
        ItemView::Reference(Reference {
            reference_id: id(value),
            source: ReferenceSource::Thread(id(8)),
            target: ItemRef::Project(id(5)).try_into().unwrap(),
            details: None,
        })
    };
    let link = |value| {
        ItemView::Link(ExternalLink {
            link_id: id(value),
            source: ReferenceSource::Thread(id(8)),
            url: "raw".to_owned(),
            title: "Link".to_owned(),
            details: None,
        })
    };
    let fixture = Fixture::new(vec![
        at(20, reference(100)),
        at(10, reference(200)),
        at(21, link(100)),
        at(11, link(200)),
    ]);
    let to = || ListQuery::ReferencesTo {
        item: ItemRef::Project(id(5)),
    };
    let (first, cursor) = fixture.list(to(), ListCursor::start(ListOrder::OldestFirst), 1);
    let (second, last) = fixture.list(to(), cursor.unwrap(), 1);
    assert_eq!(first, vec![ItemRef::Reference(id(200))]);
    assert_eq!(second, vec![ItemRef::Reference(id(100))]);
    assert_eq!(last, None);
    assert_eq!(
        fixture.all(ListQuery::ReferencesFrom {
            item: ItemRef::Thread(id(8))
        }),
        vec![ItemRef::Reference(id(200)), ItemRef::Reference(id(100))]
    );
    assert_eq!(
        fixture.all(ListQuery::Links {
            item: ItemRef::Thread(id(8))
        }),
        vec![ItemRef::Link(id(200)), ItemRef::Link(id(100))]
    );
}

#[test]
fn ported_search_keeps_literal_ascii_case_scope_and_position_order() {
    let mut other_thread = thread();
    other_thread.thread_id = id(40);
    other_thread.area_id = id(41);
    let fixture = Fixture::new(vec![
        at(1, thread_view(thread())),
        at(2, thread_view(other_thread)),
        at(5, post(50, 8, "NAVIGATION plain é")),
        at(4, post(51, 8, "Navigation 100%_ É")),
        at(3, post(52, 40, "Navigation outside")),
    ]);
    let in_area = |text| ListQuery::SearchPosts {
        scope: PostScope::Area { area_id: id(4) },
        text: search(text),
    };
    assert_eq!(
        fixture.all(in_area(" navigation ")),
        vec![ItemRef::Post(id(51)), ItemRef::Post(id(50))]
    );
    assert_eq!(fixture.all(in_area("%_")), vec![ItemRef::Post(id(51))]);
    assert_eq!(fixture.all(in_area("É")), vec![ItemRef::Post(id(51))]);
    assert_eq!(fixture.all(in_area("é")), vec![ItemRef::Post(id(50))]);
    assert_eq!(
        fixture.all(ListQuery::SearchPosts {
            scope: PostScope::Workspace,
            text: search("navigation"),
        }),
        vec![
            ItemRef::Post(id(52)),
            ItemRef::Post(id(51)),
            ItemRef::Post(id(50))
        ]
    );
    assert_eq!(
        fixture.all(ListQuery::Posts {
            scope: PostScope::Thread { thread_id: id(40) }
        }),
        vec![ItemRef::Post(id(52))]
    );
}

#[test]
fn deliverables_list_projects_the_computed_status() {
    let deliverable_view = |value| ItemView::Deliverable {
        view: crate::status_rules::deliverable_view(
            &deliverable(value),
            &[issue(31, 30, WorkStatus::Closed)],
        ),
        blocked_by: vec![],
    };
    let fixture = Fixture::new(vec![
        at(1, deliverable_view(10)),
        at(3, deliverable_view(30)),
    ]);
    let (items, _) = (
        list_answer(
            &ListQuery::Deliverables { project_id: id(5) },
            ListCursor::start(ListOrder::OldestFirst),
            limit(10),
            &ListReads {
                reader: &fixture.reader,
                items: &fixture.items,
                threads: &[],
                memberships: &[],
            },
        )
        .unwrap(),
        (),
    );
    let WorkspaceAnswer::Page { items, .. } = items else {
        panic!("page");
    };
    let statuses: Vec<_> = items
        .iter()
        .filter_map(|item| match item {
            ItemView::Deliverable { view, .. } => Some(view.status),
            _ => None,
        })
        .collect();
    assert_eq!(
        statuses,
        vec![
            workspace_model::DeliverableStatus::Open,
            workspace_model::DeliverableStatus::Closed
        ]
    );
}

#[test]
fn threads_list_by_area_or_workspace_and_followed_only_keeps_current_memberships() {
    let mut elsewhere = thread();
    elsewhere.thread_id = id(40);
    elsewhere.area_id = id(41);
    let mut left = member(context().actor, MemberRole::Participant);
    left.thread_member_id = id(60);
    left.thread_id = id(40);
    left.state = MembershipState::Left {
        at: "2026-11-13T12:00:00Z".parse().unwrap(),
    };
    let fixture = Fixture::new(vec![
        at(1, thread_view(thread())),
        at(2, thread_view(elsewhere)),
        at(
            3,
            ItemView::ThreadMember(member(context().actor, MemberRole::Observer)),
        ),
        at(4, ItemView::ThreadMember(left)),
    ]);
    let threads = |area_id, followed_only| ListQuery::Threads {
        area_id,
        followed_only,
    };
    assert_eq!(
        fixture.all(threads(None, false)),
        vec![ItemRef::Thread(id(8)), ItemRef::Thread(id(40))]
    );
    assert_eq!(
        fixture.all(threads(Some(id(41)), false)),
        vec![ItemRef::Thread(id(40))]
    );
    assert_eq!(
        fixture.all(threads(None, true)),
        vec![ItemRef::Thread(id(8))]
    );
    assert_eq!(
        fixture.all(ListQuery::ThreadMembers {
            thread_id: id(40),
            include_left: false
        }),
        vec![]
    );
    assert_eq!(
        fixture.all(ListQuery::ThreadMembers {
            thread_id: id(40),
            include_left: true
        }),
        vec![ItemRef::ThreadMember(id(60))]
    );
}

#[test]
fn roles_project_issues_work_log_tags_and_archived_areas_filter_as_named() {
    let role = |value, state| {
        ItemView::ProjectRole(ProjectRoleAssignment {
            project_role_id: id(value),
            project_id: id(5),
            role: ProjectRole::Advisor,
            holder: id(2),
            granted_at: "2026-11-13T12:00:00Z".parse().unwrap(),
            state,
        })
    };
    let tag = |text: &str| TrailTag::try_from(text.to_owned()).unwrap();
    let entry = |value, tags: Vec<TrailTag>| {
        ItemView::WorkLogEntry(WorkLogEntry {
            entry_id: id(value),
            project_id: id(5),
            author: id(2),
            body: workspace_model::WorkLogBody::Redacted,
            tags,
        })
    };
    let mut other_project_deliverable = deliverable(11);
    other_project_deliverable.project_id = id(50);
    let deliverable_view = |row| ItemView::Deliverable {
        view: crate::status_rules::deliverable_view(&row, &[]),
        blocked_by: vec![],
    };
    let mut archived = area();
    archived.area_id = id(40);
    archived.name = name("Old Area");
    archived.state = AreaState::Archived;
    let fixture = Fixture::new(vec![
        at(1, role(70, AssignmentState::Active)),
        at(
            2,
            role(
                71,
                AssignmentState::Ended {
                    at: "2026-11-13T12:00:00Z".parse().unwrap(),
                },
            ),
        ),
        at(3, deliverable_view(deliverable(10))),
        at(4, deliverable_view(other_project_deliverable)),
        at(5, ItemView::Issue(issue(20, 10, WorkStatus::Open))),
        at(6, ItemView::Issue(issue(21, 11, WorkStatus::Open))),
        at(7, entry(80, vec![tag("decision"), tag("api")])),
        at(8, entry(81, vec![tag("decision")])),
        at(9, ItemView::Area(area())),
        at(10, ItemView::Area(archived)),
        at(11, ItemView::Project(project())),
    ]);
    assert_eq!(
        fixture.all(ListQuery::ProjectRoles {
            project_id: id(5),
            include_ended: false
        }),
        vec![ItemRef::ProjectRole(id(70))]
    );
    assert_eq!(
        fixture
            .all(ListQuery::ProjectRoles {
                project_id: id(5),
                include_ended: true
            })
            .len(),
        2
    );
    assert_eq!(
        fixture.all(ListQuery::ProjectIssues { project_id: id(5) }),
        vec![ItemRef::Issue(id(20))]
    );
    assert_eq!(
        fixture.all(ListQuery::WorkLog {
            project_id: id(5),
            tags: vec![tag("decision"), tag("api")]
        }),
        vec![ItemRef::WorkLogEntry(id(80))]
    );
    assert_eq!(
        fixture
            .all(ListQuery::WorkLog {
                project_id: id(5),
                tags: vec![]
            })
            .len(),
        2
    );
    assert_eq!(
        fixture.all(ListQuery::Areas {
            include_archived: false
        }),
        vec![ItemRef::Area(id(4))]
    );
    assert_eq!(
        fixture.all(ListQuery::SearchItems {
            text: search("AREA"),
            item_kind: SearchableKind::All,
            include_archived: true,
        }),
        vec![ItemRef::Area(id(4)), ItemRef::Area(id(40))]
    );
    assert_eq!(
        fixture.all(ListQuery::SearchItems {
            text: search("project"),
            item_kind: SearchableKind::Area,
            include_archived: true,
        }),
        vec![]
    );
    assert_eq!(
        fixture.all(ListQuery::SearchItems {
            text: search("project"),
            item_kind: SearchableKind::All,
            include_archived: false,
        }),
        vec![ItemRef::Project(id(5))]
    );
}

#[test]
fn organization_lists_are_refused_inside_a_workspace_store() {
    let fixture = Fixture::new(vec![]);
    let refused = list_answer(
        &ListQuery::Workspaces {
            include_archived: false,
        },
        ListCursor::start(ListOrder::OldestFirst),
        limit(1),
        &ListReads {
            reader: &fixture.reader,
            items: &[],
            threads: &[],
            memberships: &[],
        },
    );
    assert!(matches!(refused, Err(RefusalReason::InvalidInput { .. })));
}

/// Walking every page in either direction returns each selected item once, in
/// the direction's position order, and the same call answers the same.
#[hegel::test]
fn paging_partitions_the_selection_in_the_cursors_direction(case: TestCase) {
    let count = case.draw(gs::integers::<u32>().min_value(0).max_value(30));
    let mut positions: Vec<u64> = (1..=u64::from(count)).collect();
    // A permutation of the input order: positions, not input order, decide.
    for index in (1..positions.len()).rev() {
        let other = case.draw(gs::integers::<usize>().min_value(0).max_value(index));
        positions.swap(index, other);
    }
    let items: Vec<_> = positions
        .iter()
        .map(|position| {
            let value = u32::try_from(*position).unwrap() + 100;
            at(*position, post(value, 8, "body"))
        })
        .collect();
    let fixture = Fixture::new(items);
    let order = if case.draw(gs::booleans()) {
        ListOrder::OldestFirst
    } else {
        ListOrder::NewestFirst
    };
    let size = case.draw(gs::integers::<u16>().min_value(1).max_value(8));
    let query = || ListQuery::Posts {
        scope: PostScope::Workspace,
    };

    let mut walked = Vec::new();
    let mut cursor = ListCursor::start(order);
    loop {
        let (page, next) = fixture.list(query(), cursor, size);
        assert_eq!(fixture.list(query(), cursor, size), (page.clone(), next));
        assert!(page.len() <= usize::from(size));
        walked.extend(page);
        match next {
            Some(next) => cursor = next,
            None => break,
        }
    }

    let mut expected: Vec<u64> = (1..=u64::from(count)).collect();
    if order == ListOrder::NewestFirst {
        expected.reverse();
    }
    let expected: Vec<_> = expected
        .into_iter()
        .map(|position| ItemRef::Post(id(u32::try_from(position).unwrap() + 100)))
        .collect();
    assert_eq!(walked, expected);
}
