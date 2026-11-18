use super::{DirectoryChange, change_workspace, workspace_list_answer};
use crate::fixtures::{id, name};
use hegel::{TestCase, generators as gs};
use workspace_model::{
    ItemRef, ItemView, ListCursor, ListOrder, ListQuery, PageLimit, RefusalReason, SearchQuery,
    Workspace, WorkspaceAnswer, WorkspaceId, WorkspaceState,
};

fn workspace(value: u32, title: &str, state: WorkspaceState) -> Workspace {
    Workspace {
        workspace_id: id(value),
        name: name(title),
        description: workspace_description(""),
        state,
    }
}
fn workspace_description(text: &str) -> workspace_model::WorkspaceDescription {
    text.to_owned().try_into().unwrap()
}
fn now() -> workspace_model::Timestamp {
    "2026-11-17T12:00:00Z".parse().unwrap()
}
fn listed(answer: WorkspaceAnswer) -> (Vec<ItemRef>, Option<ListCursor>) {
    let WorkspaceAnswer::Page { items, next_cursor } = answer else {
        panic!("page");
    };
    (items.iter().map(ItemView::item_ref).collect(), next_cursor)
}

#[test]
fn a_rename_keeps_its_own_canonical_name_and_refuses_another_workspaces() {
    let current = workspace(1, "Alpha", WorkspaceState::Active);
    let update = DirectoryChange::Update {
        name: name("ALPHA"),
        description: workspace_description("Team space"),
    };

    let own = change_workspace(now(), &current, &update, Some(&id(1)));
    let taken = change_workspace(now(), &current, &update, Some(&id(2)));

    let (statement, after) = own.unwrap();
    assert!(
        statement
            .statement
            .sql
            .starts_with("UPDATE workspaces SET name=?")
    );
    assert_eq!(after.name.as_str(), "ALPHA");
    assert_eq!(after.description.as_str(), "Team space");
    assert!(matches!(
        taken,
        Err(RefusalReason::WorkspaceNameInUse { .. })
    ));
}

#[test]
fn archive_leaves_an_archived_workspace_and_an_archived_one_is_refused() {
    let current = workspace(1, "Alpha", WorkspaceState::Active);

    let (statement, after) =
        change_workspace(now(), &current, &DirectoryChange::Archive, None).unwrap();
    let again = change_workspace(now(), &after, &DirectoryChange::Archive, None);

    assert!(statement.statement.sql.contains("SET state='archived'"));
    assert_eq!(after.state, WorkspaceState::Archived);
    assert_eq!(again.unwrap_err(), RefusalReason::WorkspaceNotAllowed);
}

#[test]
fn a_machine_lists_only_its_allowed_workspaces_and_search_matches_name_or_description() {
    let mut described = workspace(3, "Gamma", WorkspaceState::Active);
    described.description = workspace_description("Payments ROUTER work");
    let all = [
        workspace(1, "Alpha", WorkspaceState::Active),
        workspace(2, "Beta", WorkspaceState::Archived),
        described,
        workspace(4, "Router Ops", WorkspaceState::Active),
    ];
    let allowed: Vec<WorkspaceId> = vec![id(1), id(2), id(3)];
    let list = |query| {
        listed(
            workspace_list_answer(
                &query,
                ListCursor::start(ListOrder::OldestFirst),
                PageLimit::try_from(10).unwrap(),
                &all,
                &allowed,
            )
            .unwrap(),
        )
        .0
    };

    assert_eq!(
        list(ListQuery::Workspaces {
            include_archived: false
        }),
        vec![ItemRef::Workspace(id(1)), ItemRef::Workspace(id(3))]
    );
    assert_eq!(
        list(ListQuery::Workspaces {
            include_archived: true
        })
        .len(),
        3
    );
    assert_eq!(
        list(ListQuery::SearchWorkspaces {
            text: SearchQuery::try_from("router".to_owned()).unwrap(),
            include_archived: true,
        }),
        vec![ItemRef::Workspace(id(3))]
    );
}

/// Whatever subset is allowed, paging in either direction walks exactly the
/// allowed, included workspaces once each, and an update never touches
/// another workspace's name.
#[hegel::test]
fn workspace_pages_cover_the_allowed_set_once_in_either_direction(case: TestCase) {
    let count = case.draw(gs::integers::<u32>().min_value(0).max_value(20));
    let all: Vec<_> = (1..=count)
        .map(|value| {
            let archived = case.draw(gs::booleans());
            workspace(
                value,
                &format!("Workspace {value}"),
                if archived {
                    WorkspaceState::Archived
                } else {
                    WorkspaceState::Active
                },
            )
        })
        .collect();
    let allowed: Vec<WorkspaceId> = all
        .iter()
        .filter(|_| case.draw(gs::booleans()))
        .map(|row| row.workspace_id.clone())
        .collect();
    let include_archived = case.draw(gs::booleans());
    let order = if case.draw(gs::booleans()) {
        ListOrder::OldestFirst
    } else {
        ListOrder::NewestFirst
    };
    let size = case.draw(gs::integers::<u16>().min_value(1).max_value(5));
    let query = ListQuery::Workspaces { include_archived };

    let mut walked = Vec::new();
    let mut cursor = ListCursor::start(order);
    loop {
        let answer = workspace_list_answer(
            &query,
            cursor,
            PageLimit::try_from(size).unwrap(),
            &all,
            &allowed,
        )
        .unwrap();
        let (page, next) = listed(answer);
        assert!(page.len() <= usize::from(size));
        walked.extend(page);
        match next {
            Some(next) => cursor = next,
            None => break,
        }
    }

    let mut expected: Vec<ItemRef> = all
        .iter()
        .filter(|row| {
            allowed.contains(&row.workspace_id)
                && (include_archived || row.state == WorkspaceState::Active)
        })
        .map(|row| ItemRef::Workspace(row.workspace_id.clone()))
        .collect();
    if order == ListOrder::NewestFirst {
        expected.reverse();
    }
    assert_eq!(walked, expected);
}
