use crate::ProjectStore;
use message_board::*;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn replacement_advances_one_subject_assignment_revision()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store =
        ProjectStore::open(&directory.path().join("project.sqlite"), project.clone()).await?;
    let mut base = crate::project_store_tests::project_record(project.clone());
    base.envelope_mut().request = None;
    let subject = RoleSubject::Project(project.clone());
    let actor = base.envelope().actor.clone();
    let assignment_id = RoleAssignmentId::generate();
    let mut assignment = RoleAssignment::new(RoleAssignmentProps {
        envelope: RecordEnvelope {
            id: assignment_id.clone().into(),
            ..base.envelope().clone()
        },
        assignment_id,
        subject: subject.clone(),
        role: AuthorityRole::Lead,
        holder: actor.clone(),
        granted_by: actor.clone(),
        model: None,
        lineage: None,
        ended: None,
        owner_grant: None,
    })?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base).await?;
    assert_eq!(
        write.assignment_revision(&subject).await?,
        AssignmentRevision::INITIAL
    );
    write
        .append_record(Record::RoleAssignment(assignment.clone()))
        .await?;
    assert_eq!(write.assignment_revision(&subject).await?.get(), 1);
    let request = RequestId::generate();
    assignment.ended = Some(RoleEnd::StoodDown {
        reason: "owner replacement".into(),
    });
    assignment.envelope.request = Some(request.clone());
    write
        .append_record(Record::RoleAssignment(assignment.clone()))
        .await?;
    let next_id = RoleAssignmentId::generate();
    assignment.assignment_id = next_id.clone();
    assignment.envelope.id = next_id.into();
    assignment.ended = None;
    write
        .append_record(Record::RoleAssignment(assignment))
        .await?;
    assert_eq!(
        write.assignment_revision(&subject).await?.get(),
        2,
        "end and grant are one role change"
    );
    assert_eq!(
        write
            .assignment_revision(&RoleSubject::Workstream(WorkstreamId::generate()))
            .await?
            .get(),
        0
    );
    write.commit().await?;
    let mut snapshot = store.snapshot().await?;
    assert_eq!(snapshot.assignment_revision(&subject).await?.get(), 2);
    snapshot.finish().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn owner_grant_cannot_be_reused_after_the_previous_assignment_ended()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store =
        ProjectStore::open(&directory.path().join("project.sqlite"), project.clone()).await?;
    let base = crate::project_store_tests::project_record(project.clone());
    let actor = base.envelope().actor.clone();
    let grant_id = OwnerGrantId::generate();
    let envelope = |id: RecordId| RecordEnvelope {
        id,
        ..base.envelope().clone()
    };
    let first_id = RoleAssignmentId::generate();
    let assignment = |id: RoleAssignmentId, ended: Option<RoleEnd>| {
        RoleAssignment::new(RoleAssignmentProps {
            envelope: envelope(id.clone().into()),
            assignment_id: id,
            subject: RoleSubject::Project(project.clone()),
            role: AuthorityRole::Lead,
            holder: actor.clone(),
            granted_by: actor.clone(),
            model: None,
            lineage: None,
            ended,
            owner_grant: Some(grant_id.clone()),
        })
    };
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    write
        .append_record(Record::OwnerGrant(OwnerGrant {
            envelope: envelope(grant_id.clone().into()),
            id: grant_id.clone(),
            grant: GrantKind::ReplaceLead {
                project: project.clone(),
                new_holder: actor.clone(),
                reason: "replacement".into(),
            },
            provenance: OwnerWordsProvenance::Relayed { by: actor.clone() },
        }))
        .await?;
    write
        .append_record(Record::RoleAssignment(assignment(
            first_id.clone(),
            Some(RoleEnd::Released {}),
        )?))
        .await?;
    write.commit().await?;
    let before = store.snapshot().await?;
    let high_water = before.high_water();
    before.finish().await?;
    let mut duplicate = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    let reused = duplicate
        .append_record(Record::RoleAssignment(assignment(
            RoleAssignmentId::generate(),
            None,
        )?))
        .await;
    assert!(
        reused.is_err(),
        "an ended assignment must not free its owner grant"
    );
    duplicate.rollback().await?;
    let mut after = store.snapshot().await?;
    assert_eq!(
        after.high_water(),
        high_water,
        "rejected reuse consumes no feed position"
    );
    assert_eq!(after.records(RecordKind::RoleAssignment).await?.len(), 1);
    after.finish().await?;
    store.close().await;
    Ok(())
}
