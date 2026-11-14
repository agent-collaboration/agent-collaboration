use super::{StatusReads, deliverable_status, deliverable_view, set_work_status};
use crate::fixtures::{context, deliverable, description, id, issue, name, project};
use central_storage::SqlValue;
use workspace_model::{
    BlockerStatus, DeliverableStatus, Initiative, ItemRef, RefusalReason, StatusCommand, WorkStatus,
};

#[test]
fn r33_a_d_deliverable_is_computed_empty_on_hold_all_closed() {
    let deliverable = deliverable(10);
    assert_eq!(
        deliverable_view(&deliverable, &[]).status,
        DeliverableStatus::Open
    );
    assert_eq!(
        deliverable_status(&deliverable, &[issue(11, 10, WorkStatus::OnHold)]),
        DeliverableStatus::Open
    );
    assert_eq!(
        deliverable_status(
            &deliverable,
            &[
                issue(11, 10, WorkStatus::Closed),
                issue(12, 10, WorkStatus::Closed)
            ]
        ),
        DeliverableStatus::Closed
    );
    assert_eq!(
        deliverable_status(
            &deliverable,
            &[
                issue(11, 10, WorkStatus::Closed),
                issue(12, 10, WorkStatus::Open)
            ]
        ),
        DeliverableStatus::Open
    );
}
#[test]
fn r33_a_issue_statuses_and_reopen_have_guarded_values() {
    let context = context();
    let project = project();
    let children = [deliverable(10)];
    let transitions = [
        (WorkStatus::Backlog, WorkStatus::Open, "backlog", "open"),
        (WorkStatus::Open, WorkStatus::OnHold, "open", "onHold"),
        (WorkStatus::OnHold, WorkStatus::Closed, "onHold", "closed"),
        (WorkStatus::Closed, WorkStatus::Open, "closed", "open"),
    ];
    for (before, after, old_value, new_value) in transitions {
        let row = issue(11, 10, before);
        let output = set_work_status(
            &context,
            &StatusCommand::SetIssue {
                issue_id: id(11),
                status: after,
            },
            StatusReads {
                issue: Some(&row),
                project: Some(&project),
                deliverables: &children,
                ..Default::default()
            },
        )
        .unwrap();
        let mutation = &output.batch.statements.first().unwrap().statement;
        assert_eq!(
            mutation.sql,
            "UPDATE issues SET status=?,updated_at=? WHERE issue_id=? AND status=?"
        );
        assert_eq!(
            mutation.args,
            vec![
                SqlValue::Text(new_value.to_owned()),
                SqlValue::Text("2026-11-13T12:00:00+00:00".to_owned()),
                SqlValue::Text("01890f2e-7b4c-7cc0-98c4-00000000000b".to_owned()),
                SqlValue::Text(old_value.to_owned())
            ]
        );
    }
}
#[test]
fn r33_b_d_project_closure_names_deliverable_and_issue_including_empty() {
    let context = context();
    let project = project();
    let children = [deliverable(10)];
    let issues = [issue(11, 10, WorkStatus::OnHold)];
    let command = StatusCommand::SetProject {
        project_id: id(5),
        status: WorkStatus::Closed,
    };
    let result = set_work_status(
        &context,
        &command,
        StatusReads {
            project: Some(&project),
            deliverables: &children,
            issues: &issues,
            ..Default::default()
        },
    );
    let blockers = match result.unwrap_err() {
        RefusalReason::ChildrenNotClosed { blockers } => blockers,
        _ => Vec::new(),
    };
    assert_eq!(blockers.len(), 2);
    assert!(blockers.iter().any(|row| row.item == ItemRef::Issue(id(11))
        && row.title == "Issue 11"
        && row.status == BlockerStatus::Work(WorkStatus::OnHold)));
    assert!(
        blockers
            .iter()
            .any(|row| row.item == ItemRef::Deliverable(id(10)) && row.title == "Deliverable 10")
    );
    assert!(matches!(
        set_work_status(
            &context,
            &command,
            StatusReads {
                project: Some(&project),
                deliverables: &children,
                ..Default::default()
            }
        ),
        Err(RefusalReason::ChildrenNotClosed { blockers }) if blockers.iter().any(|row| row.item == ItemRef::Deliverable(id(10)) && row.title == "Deliverable 10")
    ));
    let issues = [issue(11, 10, WorkStatus::Closed)];
    assert!(
        set_work_status(
            &context,
            &command,
            StatusReads {
                project: Some(&project),
                deliverables: &children,
                issues: &issues,
                ..Default::default()
            }
        )
        .is_ok()
    );
}
#[test]
fn r33_c_initiative_close_and_both_closed_parent_reopens_are_refused() {
    let context = context();
    let mut project = project();
    let children = [deliverable(10)];
    let row = issue(11, 10, WorkStatus::Closed);
    let initiative = Initiative {
        initiative_id: id(20),
        name: name("Initiative A"),
        description: description("Goal"),
        status: WorkStatus::Open,
    };
    assert!(
        matches!(set_work_status(&context,&StatusCommand::SetInitiative {initiative_id:id(20),status:WorkStatus::Closed},StatusReads {initiative:Some(&initiative),initiative_projects:std::slice::from_ref(&project),..Default::default()}),Err(RefusalReason::ChildrenNotClosed {blockers}) if blockers.iter().any(|blocker|blocker.title == "Project X"))
    );
    project.status = WorkStatus::Closed;
    assert!(
        matches!(set_work_status(&context,&StatusCommand::SetIssue {issue_id:id(11),status:WorkStatus::Open},StatusReads {issue:Some(&row),project:Some(&project),deliverables:&children,..Default::default()}),Err(RefusalReason::ParentClosed {parent}) if parent.title == "Project X")
    );
    let closed = Initiative {
        status: WorkStatus::Closed,
        ..initiative
    };
    assert!(
        matches!(set_work_status(&context,&StatusCommand::SetProject {project_id:id(5),status:WorkStatus::Open},StatusReads {project:Some(&project),project_initiatives:&[closed],..Default::default()}),Err(RefusalReason::ParentClosed {parent}) if parent.title == "Initiative A")
    );
}
