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
            first_post: crate::fixtures::first_post(31, 32),
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
                first_post: crate::fixtures::first_post(31, 32),
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

#[test]
fn updates_rename_only_their_existing_row_and_file_project_changes_under_the_project() {
    let context = context();
    let area = area();
    let project = crate::fixtures::project();
    let deliverable = crate::fixtures::deliverable(10);
    let issue = crate::fixtures::issue(20, 10, workspace_model::WorkStatus::Open);
    let cases = [
        (
            OrganizationCommand::UpdateArea {
                area_id: id(4),
                name: name("Renamed"),
            },
            "UPDATE areas SET name=?",
            None,
        ),
        (
            OrganizationCommand::UpdateProject {
                project_id: id(5),
                title: name("Renamed"),
            },
            "UPDATE projects SET title=?",
            Some(id::<workspace_model::ProjectId>(5)),
        ),
        (
            OrganizationCommand::UpdateIssue {
                issue_id: id(20),
                title: workspace_model::IssueTitle::try_from("Renamed".to_owned()).unwrap(),
            },
            "UPDATE issues SET title=?",
            Some(id::<workspace_model::ProjectId>(5)),
        ),
    ];
    for (command, sql, project_column) in cases {
        let missing = organize_work(&context, &command, OrganizationReads::default());
        let plan = organize_work(
            &context,
            &command,
            OrganizationReads {
                area: Some(&area),
                project: Some(&project),
                deliverable: Some(&deliverable),
                issue: Some(&issue),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(matches!(missing, Err(RefusalReason::NotFound { .. })));
        let statements = &plan.batch.statements;
        assert_eq!(statements.len(), 2);
        assert!(statements[0].statement.sql.starts_with(sql));
        assert_eq!(
            statements[0].statement.args[0],
            central_storage::SqlValue::Text("Renamed".to_owned())
        );
        assert_eq!(
            statements[1].statement.args[5],
            project_column.map_or(central_storage::SqlValue::Null, |id| {
                central_storage::SqlValue::Text(id.to_string())
            })
        );
        assert!(matches!(
            plan.answer,
            workspace_model::RuleAnswer::Updated { .. }
        ));
    }
}

#[test]
fn an_area_update_refuses_another_workspaces_area() {
    let mut foreign = area();
    foreign.workspace_id = id(99);
    let refused = organize_work(
        &context(),
        &OrganizationCommand::UpdateArea {
            area_id: id(4),
            name: name("Renamed"),
        },
        OrganizationReads {
            area: Some(&foreign),
            ..Default::default()
        },
    );
    assert!(matches!(refused, Err(RefusalReason::NotFound { .. })));
}

/// Every name the vocabulary admits is written verbatim; anything it refuses
/// never reaches a rule; the same update always plans the same batch.
#[hegel::test]
fn update_rules_write_admitted_names_verbatim_and_deterministically(case: hegel::TestCase) {
    use hegel::generators as gs;
    let raw: String = case.draw(gs::text().max_size(300));
    let which = case.draw(gs::integers::<u8>().min_value(0).max_value(2));
    let area = area();
    let project = crate::fixtures::project();
    let issue = crate::fixtures::issue(20, 10, workspace_model::WorkStatus::Open);
    let reads = || OrganizationReads {
        area: Some(&area),
        project: Some(&project),
        issue: Some(&issue),
        ..Default::default()
    };
    let drawn = match which {
        0 => workspace_model::ResourceName::try_from(raw.clone())
            .ok()
            .map(|name| {
                (
                    name.as_str().to_owned(),
                    OrganizationCommand::UpdateArea {
                        area_id: id(4),
                        name,
                    },
                )
            }),
        1 => workspace_model::ResourceName::try_from(raw.clone())
            .ok()
            .map(|title| {
                (
                    title.as_str().to_owned(),
                    OrganizationCommand::UpdateProject {
                        project_id: id(5),
                        title,
                    },
                )
            }),
        _ => workspace_model::IssueTitle::try_from(raw.clone())
            .ok()
            .map(|title| {
                (
                    title.as_str().to_owned(),
                    OrganizationCommand::UpdateIssue {
                        issue_id: id(20),
                        title,
                    },
                )
            }),
    };
    let Some((admitted, command)) = drawn else {
        // Refused at the vocabulary boundary: blank, too long or not admitted text.
        assert!(raw.trim().is_empty() || raw.len() > 256 || raw.chars().any(char::is_control));
        return;
    };

    let first = organize_work(&context(), &command, reads()).unwrap();
    let second = organize_work(&context(), &command, reads()).unwrap();

    assert_eq!(first, second);
    let written = match &first.batch.statements[0].statement.args[0] {
        central_storage::SqlValue::Text(value) => value.clone(),
        other => panic!("name must be text: {other:?}"),
    };
    assert_eq!(written, admitted);
}
