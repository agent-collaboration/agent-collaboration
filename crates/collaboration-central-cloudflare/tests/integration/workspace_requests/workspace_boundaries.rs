use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
#[tokio::test]
async fn two_workspaces_share_one_actor_but_never_rows_or_foreign_area_placement() {
    let f = Fixture::new().await;
    let first_area = f.area(800).await;
    let other = call(
        &f.host,
        &f.caller,
        None,
        801,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Other workspace"),
        },
    )
    .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace: other }) =
        other.result
    else {
        panic!("workspace");
    };
    let runtime = f.host.central(&f.caller.organization_id).await;
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
                        args: vec![
                            text(
                                serde_json::to_string(&[&f.workspace, &other.workspace_id])
                                    .unwrap()
                            ),
                            text(f.caller.caller.machine_id())
                        ]
                    },
                    expect: RowExpectation::Exactly(1)
                }],
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    let m::ItemRef::Area(second_area) = created(
        call(
            &f.host,
            &f.caller,
            Some(other.workspace_id.clone()),
            802,
            m::WorkspaceOperation::CreateArea {
                name: name("Second area"),
            },
        )
        .await,
    ) else {
        panic!("area");
    };
    for (request, operation, item) in [
        (
            803,
            m::WorkspaceOperation::CreateThread {
                area_id: second_area.clone(),
                title: name("Foreign"),
            },
            m::ItemRef::Area(second_area.clone()),
        ),
        (
            804,
            m::WorkspaceOperation::AddBacklogItem {
                area_id: second_area.clone(),
                title: name("Foreign"),
                description: "No foreign area".to_owned().try_into().unwrap(),
            },
            m::ItemRef::Area(second_area.clone()),
        ),
        (
            805,
            m::WorkspaceOperation::CreateProject {
                area_id: id(999),
                title: name("Missing"),
                lead: None,
            },
            m::ItemRef::Area(id(999)),
        ),
    ] {
        assert_eq!(
            f.call(request, operation).await.result,
            m::WorkspaceResult::NotFound {
                workspace_id: f.workspace.clone(),
                item
            }
        );
    }
    let thread = f.thread(806, first_area).await;
    let other_thread = created(
        call(
            &f.host,
            &f.caller,
            Some(other.workspace_id.clone()),
            807,
            m::WorkspaceOperation::CreateThread {
                area_id: second_area,
                title: name("Other thread"),
            },
        )
        .await,
    );
    let first = f
        .call(
            0,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::Thread(thread.clone()),
            },
        )
        .await;
    let second = call(
        &f.host,
        &f.caller,
        Some(other.workspace_id.clone()),
        0,
        m::WorkspaceOperation::Show { item: other_thread },
    )
    .await;
    assert_eq!(first.actors, second.actors);
    assert_eq!(first.actors.len(), 1);
    let actors = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT actor_identity_id FROM actor_identities",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(actors.rows.len(), 1);
    let target = m::ReferenceTarget::try_from(m::ItemRef::Thread(thread)).unwrap();
    let m::ItemRef::Area(third_area) = created(
        call(
            &f.host,
            &f.caller,
            Some(other.workspace_id.clone()),
            809,
            m::WorkspaceOperation::CreateArea {
                name: name("Third area"),
            },
        )
        .await,
    ) else {
        panic!("area");
    };
    let m::ItemRef::Thread(source_thread) = created(
        call(
            &f.host,
            &f.caller,
            Some(other.workspace_id.clone()),
            808,
            m::WorkspaceOperation::CreateThread {
                area_id: third_area,
                title: name("Reference source"),
            },
        )
        .await,
    ) else {
        panic!("thread");
    };
    let source = m::ReferenceSource::Thread(source_thread);
    let refused = call(
        &f.host,
        &f.caller,
        Some(other.workspace_id.clone()),
        810,
        m::WorkspaceOperation::AddReference {
            source,
            target: target.clone(),
            details: None,
        },
    )
    .await;
    assert_eq!(
        refused.result,
        m::WorkspaceResult::NotFound {
            workspace_id: other.workspace_id,
            item: target.into()
        }
    );
    let missing_area = f.area(813).await;
    let missing_thread = f.thread(812, missing_area).await;
    let missing = f
        .call(
            811,
            m::WorkspaceOperation::AddReference {
                source: m::ReferenceSource::Thread(missing_thread),
                target: m::ReferenceTarget::try_from(m::ItemRef::Area(id(999))).unwrap(),
                details: None,
            },
        )
        .await;
    assert_eq!(
        missing.result,
        m::WorkspaceResult::NotFound {
            workspace_id: f.workspace.clone(),
            item: m::ItemRef::Area(id(999))
        }
    );
    f.host.close().await;
}
#[tokio::test]
async fn backlog_removal_cascades_both_reference_directions_and_literal_link_text_is_kept() {
    let f = Fixture::new().await;
    let area = f.area(820).await;
    let project = f.project(821, area.clone()).await;
    let thread = f.thread(822, area.clone()).await;
    let m::ItemRef::BacklogItem(item) = created(
        f.call(
            823,
            m::WorkspaceOperation::AddBacklogItem {
                area_id: area.clone(),
                title: name("Backlog"),
                description: "Description".to_owned().try_into().unwrap(),
            },
        )
        .await,
    ) else {
        panic!("backlog");
    };
    let source = m::ReferenceSource::BacklogItem(item.clone());
    created(
        f.call(
            824,
            m::WorkspaceOperation::AddReference {
                source: source.clone(),
                target: m::ReferenceTarget::try_from(m::ItemRef::Project(project)).unwrap(),
                details: None,
            },
        )
        .await,
    );
    created(
        f.call(
            825,
            m::WorkspaceOperation::AddReference {
                source: m::ReferenceSource::Thread(thread),
                target: m::ReferenceTarget::try_from(m::ItemRef::BacklogItem(item.clone()))
                    .unwrap(),
                details: None,
            },
        )
        .await,
    );
    let second_thread = f.thread(829, area.clone()).await;
    created(
        f.call(
            830,
            m::WorkspaceOperation::AddReference {
                source: m::ReferenceSource::Thread(second_thread),
                target: m::ReferenceTarget::try_from(m::ItemRef::BacklogItem(item.clone()))
                    .unwrap(),
                details: None,
            },
        )
        .await,
    );
    let reverse = f
        .call(
            0,
            m::WorkspaceOperation::List {
                query: m::ListQuery::ReferencesTo {
                    item: m::ItemRef::BacklogItem(item.clone()),
                },
                after: m::ChangePosition::ZERO,
                limit: 10.try_into().unwrap(),
            },
        )
        .await;
    assert!(
        matches!(reverse.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Page{items,..}) if items.len()==2)
    );
    let literal = created(
        f.call(
            826,
            m::WorkspaceOperation::AddLink {
                source,
                url: "not a URL".to_owned(),
                title: "Literal".to_owned(),
                details: None,
            },
        )
        .await,
    );
    let shown = f
        .call(0, m::WorkspaceOperation::Show { item: literal })
        .await;
    assert!(
        matches!(shown.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Item{item:m::ItemView::Link(link)}) if link.url=="not a URL")
    );
    f.call(
        827,
        m::WorkspaceOperation::RemoveBacklogItem { item_id: item },
    )
    .await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let rows = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT reference_id FROM item_references",
                args: vec![],
            },
        )
        .await
        .unwrap();
    assert!(rows.rows.is_empty());
    let empty = f
        .call(
            828,
            m::WorkspaceOperation::AddBacklogItem {
                area_id: area,
                title: name("Empty"),
                description: "".to_owned().try_into().unwrap(),
            },
        )
        .await;
    assert_eq!(
        empty.result,
        m::WorkspaceResult::Refused(m::RefusalReason::EmptyDescription)
    );
    f.host.close().await;
}

#[tokio::test]
async fn host_stamped_person_uses_the_same_allowance_and_has_a_distinct_typed_identity() {
    let f = Fixture::new().await;
    let area = f.area(830).await;
    let mut person = f.caller.clone();
    person.caller = m::Caller::Person {
        machine_id: f.caller.caller.machine_id().clone(),
        person_id: id(831),
    };
    let operation = m::WorkspaceOperation::CreateThread {
        area_id: area,
        title: name("Person's thread"),
    };
    let created_reply = call(
        &f.host,
        &person,
        Some(f.workspace.clone()),
        832,
        operation.clone(),
    )
    .await;
    let item = created(created_reply.clone());
    let shown = call(
        &f.host,
        &person,
        Some(f.workspace.clone()),
        0,
        m::WorkspaceOperation::Show { item },
    )
    .await;
    assert_eq!(shown.actors.len(), 1);
    assert_eq!(
        shown.actors[0].identity,
        m::Identity::Person { person_id: id(831) }
    );
    assert_eq!(
        created_reply,
        call(
            &f.host,
            &person,
            Some(f.workspace.clone()),
            832,
            operation.clone()
        )
        .await
    );
    assert_eq!(
        call(
            &f.host,
            &f.caller,
            Some(f.workspace.clone()),
            832,
            operation
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::RequestIdReused)
    );
    // This context is fixture-stamped. Person sign-in and Host refusal are C/slice 4.
    f.host.close().await;
}
