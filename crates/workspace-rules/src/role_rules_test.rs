use super::{RoleReads, change_role};
use crate::fixtures::{context, deliverable, id, project};
use central_storage::SqlValue;
use workspace_model::{DeliverableRole, ProjectRole, RefusalReason, RoleCommand};
#[test]
fn r3_b_e_lead_required_and_advisor_replacement_is_one_batch() {
    let mut context = context();
    let project = project();
    let reads = || RoleReads {
        project: &project,
        deliverable: None,
        project_assignment: None,
        deliverable_assignment: None,
    };
    assert!(matches!(
        change_role(
            &context,
            &RoleCommand::ReleaseProject {
                project_id: id(5),
                role: ProjectRole::Lead
            },
            reads()
        ),
        Err(RefusalReason::LeadRequired { .. })
    ));
    let command = RoleCommand::GrantProject {
        assignment_id: id(10),
        project_id: id(5),
        role: ProjectRole::Advisor,
        holder: id(20),
    };
    let output = change_role(&context, &command, reads()).unwrap();
    let sql: Vec<_> = output
        .batch
        .statements
        .iter()
        .map(|row| row.statement.sql)
        .collect();
    assert!(
        sql.first()
            .is_some_and(|row| row.starts_with("UPDATE project_roles SET ended_at="))
    );
    assert!(
        sql.get(1)
            .is_some_and(|row| row.starts_with("INSERT INTO project_roles"))
    );
    assert!(
        output
            .batch
            .statements
            .last()
            .unwrap()
            .statement
            .args
            .contains(&SqlValue::Text(
                id::<workspace_model::ActorIdentityId>(20).to_string()
            ))
    );
    context.actor = id(99);
    assert!(matches!(
        change_role(&context, &command, reads()),
        Err(RefusalReason::NotLead { .. })
    ));
}
#[test]
fn r20_deliverable_grant_is_scoped_to_only_its_deliverable() {
    let context = context();
    let project = project();
    let row = deliverable(10);
    let output = change_role(
        &context,
        &RoleCommand::GrantDeliverable {
            assignment_id: id(20),
            deliverable_id: id(10),
            role: DeliverableRole::Implementer,
            holder: id(30),
        },
        RoleReads {
            project: &project,
            deliverable: Some(&row),
            project_assignment: None,
            deliverable_assignment: None,
        },
    )
    .unwrap();
    assert!(
        output
            .batch
            .statements
            .first()
            .unwrap()
            .statement
            .sql
            .contains("WHERE deliverable_id=? AND role=?")
    );
}
