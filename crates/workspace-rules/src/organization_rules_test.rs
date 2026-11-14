use super::{OrganizationReads, organize_work};
use crate::fixtures::{area, context, id, name};
use workspace_model::{OrganizationCommand, RefusalReason};
#[test]
fn r1_a_b_r3_a_placement_and_project_lead_are_one_plan() {
    let context = context();
    let mut area = area();
    let command = OrganizationCommand::CreateProject {
        project_id: id(5),
        area_id: id(4),
        title: name("Project"),
        lead: id(2),
        role_id: id(30),
    };
    assert!(matches!(
        organize_work(&context, &command, OrganizationReads::default()),
        Err(RefusalReason::NotFound { .. })
    ));
    area.workspace_id = id(99);
    assert!(matches!(
        organize_work(
            &context,
            &command,
            OrganizationReads {
                area: Some(&area),
                ..Default::default()
            }
        ),
        Err(RefusalReason::NotFound { .. })
    ));
    area.workspace_id = context.workspace_id.clone();
    let plan = organize_work(
        &context,
        &command,
        OrganizationReads {
            area: Some(&area),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(plan.batch.statements.len(), 3);
    assert!(
        plan.batch
            .statements
            .first()
            .unwrap()
            .statement
            .sql
            .starts_with("INSERT INTO projects")
    );
    assert!(
        plan.batch
            .statements
            .get(1)
            .unwrap()
            .statement
            .sql
            .contains("'lead'")
    );
    assert!(
        plan.batch
            .statements
            .last()
            .unwrap()
            .statement
            .sql
            .contains("MAX(position)")
    );
}
#[test]
fn r6_issue_without_deliverable_is_not_planned() {
    let command = OrganizationCommand::CreateIssue {
        issue_id: id(10),
        deliverable_id: id(11),
        title: workspace_model::IssueTitle::try_from("Issue".to_owned()).unwrap(),
    };
    assert!(matches!(
        organize_work(&context(), &command, OrganizationReads::default()),
        Err(RefusalReason::NotFound { .. })
    ));
}

#[test]
fn r1_b_thread_and_backlog_cannot_use_another_workspaces_area() {
    let context = context();
    let mut area = area();
    area.workspace_id = id(99);
    let thread = crate::change_thread(
        &context,
        &workspace_model::ThreadCommand::CreateThread {
            thread_id: id(8),
            member_id: id(9),
            area_id: id(4),
            title: name("Thread"),
        },
        crate::ThreadReads {
            area: Some(&area),
            ..Default::default()
        },
    );
    assert_eq!(
        thread.unwrap_err(),
        RefusalReason::NotFound {
            item: workspace_model::ItemRef::Area(id(4))
        }
    );
    let backlog = crate::change_backlog(
        &context,
        &workspace_model::BacklogCommand::Add {
            item_id: id(10),
            area_id: id(4),
            title: name("Item"),
            description: crate::fixtures::description("Details"),
        },
        crate::BacklogReads {
            area: Some(&area),
            item: None,
            ..Default::default()
        },
    );
    assert_eq!(
        backlog.unwrap_err(),
        RefusalReason::NotFound {
            item: workspace_model::ItemRef::Area(id(4))
        }
    );
}

#[test]
fn e7_a_rules_in_two_workspaces_use_the_same_resolved_identity_without_registry_writes() {
    let first = context();
    let mut second = context();
    second.workspace_id = id(99);
    for context in [first, second] {
        let mut area = area();
        area.workspace_id = context.workspace_id.clone();
        let output = crate::change_thread(
            &context,
            &workspace_model::ThreadCommand::CreateThread {
                thread_id: id(8),
                member_id: id(9),
                area_id: id(4),
                title: name("Thread"),
            },
            crate::ThreadReads {
                area: Some(&area),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            output
                .batch
                .statements
                .iter()
                .all(|row| !row.statement.sql.contains("actor_identities"))
        );
        assert!(
            output
                .batch
                .statements
                .first()
                .unwrap()
                .statement
                .args
                .contains(&central_storage::SqlValue::Text(context.actor.to_string()))
        );
    }
}

#[test]
fn r33_c_creating_an_open_child_in_a_closed_project_names_the_parent() {
    let mut project = crate::fixtures::project();
    project.status = workspace_model::WorkStatus::Closed;
    let deliverable = crate::fixtures::deliverable(10);
    let commands = [
        OrganizationCommand::CreateDeliverable {
            deliverable_id: id(11),
            project_id: id(5),
            title: name("New"),
        },
        OrganizationCommand::CreateIssue {
            issue_id: id(12),
            deliverable_id: id(10),
            title: workspace_model::IssueTitle::try_from("New".to_owned()).unwrap(),
        },
    ];
    for command in commands {
        let result = organize_work(
            &context(),
            &command,
            OrganizationReads {
                project: Some(&project),
                deliverable: Some(&deliverable),
                ..Default::default()
            },
        );
        assert!(
            matches!(result,Err(RefusalReason::ParentClosed {parent}) if parent.title == "Project X")
        );
    }
    project.status = workspace_model::WorkStatus::Open;
    assert!(
        organize_work(
            &context(),
            &OrganizationCommand::CreateIssue {
                issue_id: id(12),
                deliverable_id: id(10),
                title: workspace_model::IssueTitle::try_from("New".to_owned()).unwrap()
            },
            OrganizationReads {
                project: Some(&project),
                deliverable: Some(&deliverable),
                ..Default::default()
            }
        )
        .is_ok()
    );
}
