//! Many-role holders unrelated to a rule are not its write operands.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::json;
use std::collections::BTreeMap;

async fn role_operand_costs(size: usize) -> Result<BTreeMap<&'static str, ReadCost>, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, stream) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut write = store.begin_admission(fixture.record_origin()?).await?;
    let template = write
        .record(&project.clone().into())
        .await?
        .ok_or("project body")?;
    for index in 0..size {
        for (subject, role) in [
            (
                RoleSubject::Project(project.clone()),
                AuthorityRole::Advisor,
            ),
            (
                RoleSubject::Workstream(stream.clone()),
                AuthorityRole::Operator,
            ),
        ] {
            let assignment = RoleAssignmentId::generate();
            let mut envelope = template.envelope().clone();
            envelope.request = None;
            envelope.id = assignment.clone().into();
            let holder = RecordedActor::declared(Identity::Human {
                human_id: format!("unrelated-{index}").try_into()?,
            });
            write
                .append_record(Record::RoleAssignment(RoleAssignment::new(
                    RoleAssignmentProps {
                        envelope: envelope.clone(),
                        assignment_id: assignment,
                        subject,
                        role,
                        holder,
                        granted_by: envelope.actor,
                        model: None,
                        lineage: None,
                        ended: None,
                        owner_grant: None,
                    },
                )?))
                .await?;
        }
        let other_stream = WorkstreamId::generate();
        let assignment = RoleAssignmentId::generate();
        let mut envelope = template.envelope().clone();
        envelope.request = None;
        envelope.id = other_stream.clone().into();
        write
            .append_record(Record::Workstream(Workstream {
                envelope: envelope.clone(),
                workstream_id: other_stream.clone(),
                project_id: project.clone(),
                name: format!("Other operator stream {index}").try_into()?,
                goal: "Irrelevant to the owner's lead rule".into(),
                state: WorkstreamState::Open {},
                revision: SubjectRevision::INITIAL,
            }))
            .await?;
        envelope.id = assignment.clone().into();
        write
            .append_record(Record::RoleAssignment(RoleAssignment::new(
                RoleAssignmentProps {
                    envelope: envelope.clone(),
                    assignment_id: assignment,
                    subject: RoleSubject::Workstream(other_stream),
                    role: AuthorityRole::Operator,
                    holder: envelope.actor.clone(),
                    granted_by: envelope.actor,
                    model: None,
                    lineage: None,
                    ended: None,
                    owner_grant: None,
                },
            )?))
            .await?;
    }
    write.commit().await?;
    let mut costs = BTreeMap::new();
    let before = current_cost();
    accepted(&fixture.api.call("workstream_rename", fixture.request(&project, json!({"kind":"workstreamRename","workstreamId":stream,"name":"Only the lead is authority","expectedRevision":0}))).await?);
    costs.insert("rename", current_cost().since(before));
    for (family, subject, role) in [
        (
            "advisor",
            RoleSubject::Project(project.clone()),
            AuthorityRole::Advisor,
        ),
        (
            "operator",
            RoleSubject::Workstream(stream.clone()),
            AuthorityRole::Operator,
        ),
    ] {
        let mut read = store.snapshot().await?;
        let revision = read.assignment_revision(&subject).await?;
        read.finish().await?;
        let holder = Identity::Human {
            human_id: format!("selected-{family}").try_into()?,
        };
        let before = current_cost();
        let grant = fixture.api.call("role_grant", fixture.request(&project, json!({"kind":"roleGrant","subject":subject,"role":role,"holder":holder,"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))).await?;
        costs.insert(family, current_cost().since(before));
        accepted(&grant);
        let key: RecordRef = serde_json::from_value(
            grant
                .pointer("/result/result/decision/effects/entries/0/0")
                .ok_or("role key")?
                .clone(),
        )?;
        let mut read = store.snapshot().await?;
        let Some(Record::RoleAssignment(assignment)) = read.record(&key).await? else {
            return Err("role body".into());
        };
        let revision = read.assignment_revision(&subject).await?;
        read.finish().await?;
        let before = current_cost();
        accepted(&fixture.api.call("role_release", fixture.request(&project, json!({"kind":"roleRelease","subject":subject,"role":role,"holder":assignment.holder,"expectedRevision":revision}))).await?);
        costs.insert(
            if family == "advisor" {
                "advisorRelease"
            } else {
                "operatorRelease"
            },
            current_cost().since(before),
        );
    }
    let before = current_cost();
    accepted(&fixture.api.call("work_trail_add", fixture.request(&project, json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":null,"text":"Implicit lead subject","tags":[],"links":[]}))).await?);
    costs.insert("implicitTrail", current_cost().since(before));
    let mut request = fixture.header();
    request.as_object_mut().ok_or("header")?.extend(json!({"projectId":project,"postId":PostId::generate(),"about":{"kind":"project","subject":{"line":"One role witness"}},"text":"Project discussion","references":[],"role":null,"watch":false,"addressees":[]}).as_object().ok_or("fields")?.clone());
    let before = current_cost();
    accepted(&fixture.api.call("thread_create", request).await?);
    costs.insert("projectThread", current_cost().since(before));
    fixture.stop().await?;
    Ok(costs)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn unrelated_advisor_and_operator_holders_do_not_grow_write_operands() -> Result<(), TestError>
{
    read_cost_counter::install();
    let small = role_operand_costs(8).await?;
    let large = role_operand_costs(80).await?;
    eprintln!("Many-role operand costs: small={small:?} large={large:?}");
    assert_eq!(small, large, "unrelated holder bodies became operands");
    assert!(
        large
            .values()
            .all(|cost| cost.statements > 0 && cost.records > 0)
    );
    Ok(())
}
