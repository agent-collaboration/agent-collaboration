use crate::ProjectStore;
use message_board::*;
use sqlx::{ConnectOptions, Connection, migrate::Migrator};

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
    let mut write = store.begin_admission().await?;
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
    let mut write = store.begin_admission().await?;
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
    let mut duplicate = store.begin_admission().await?;
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

#[tokio::test]
async fn populated_role_table_is_preserved_when_owner_grants_are_added()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("project.sqlite");
    let project = ProjectId::generate();
    let assignment_id = RoleAssignmentId::generate();
    let mut base = crate::project_store_tests::project_record(project.clone());
    base.envelope_mut().request = None;
    let epoch = EpochNumber::try_from(1)?;
    base.envelope_mut().position = RecordPosition::Project(FeedCursor {
        epoch,
        position: FeedPosition::try_from(1)?,
    });
    let actor = base.envelope().actor.clone();
    let assignment = Record::RoleAssignment(RoleAssignment::new(RoleAssignmentProps {
        envelope: RecordEnvelope {
            id: assignment_id.clone().into(),
            position: RecordPosition::Project(FeedCursor {
                epoch,
                position: FeedPosition::try_from(2)?,
            }),
            ..base.envelope().clone()
        },
        assignment_id: assignment_id.clone(),
        subject: RoleSubject::Project(project.clone()),
        role: AuthorityRole::Lead,
        holder: actor.clone(),
        granted_by: actor,
        model: None,
        lineage: None,
        ended: None,
        owner_grant: None,
    })?);
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .connect()
        .await?;
    let prefix = Migrator::with_migrations(
        crate::PROJECT_MIGRATOR
            .iter()
            .filter(|step| step.version <= 202610120002)
            .cloned()
            .collect(),
    );
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    prefix.run_direct(None, &mut *transaction, false).await?;
    for (kind, record) in [("project", &base), ("roleAssignment", &assignment)] {
        let reference = record.reference();
        sqlx::query("INSERT INTO record_identities(kind, id, payload) VALUES (?, ?, ?)")
            .bind(kind)
            .bind(reference.id.as_str())
            .bind(serde_json::to_string(record)?)
            .execute(&mut *transaction)
            .await?;
        let RecordPosition::Project(cursor) = record.envelope().position else {
            panic!("project position")
        };
        let entry = FeedEntry {
            epoch,
            position: cursor.position,
            record: reference.clone(),
            request: None,
            router_time: record.envelope().router_time,
            actor: record.envelope().actor.clone(),
            content: FeedContent::Shown(Box::new(record.clone())),
        };
        sqlx::query("INSERT INTO project_feed(position, epoch, record_kind, record_id, entry) VALUES (?, 1, ?, ?, ?)")
            .bind(i64::try_from(cursor.position.get())?).bind(kind).bind(reference.id.as_str()).bind(serde_json::to_string(&entry)?).execute(&mut *transaction).await?;
    }
    sqlx::query("INSERT INTO project_state(project_id) VALUES (?)")
        .bind(project.as_str())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("INSERT INTO role_assignments(assignment_id, project_subject, role, active) VALUES (?, ?, 'lead', 1)")
        .bind(assignment_id.as_str()).bind(project.as_str()).execute(&mut *transaction).await?;
    sqlx::query("UPDATE feed_checkpoint SET last_position = 2")
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    connection.close().await?;
    let migrated = ProjectStore::open(&path, project).await?;
    assert!(matches!(
        migrated.schema_state(),
        crate::StoreSchemaState::Ready {
            version: crate::project_schema::PROJECT_SCHEMA_VERSION,
            ..
        }
    ));
    let mut snapshot = migrated.snapshot().await?;
    assert_eq!(
        snapshot.record(&assignment_id.into()).await?,
        Some(assignment.clone())
    );
    assert_eq!(
        snapshot.records(RecordKind::RoleAssignment).await?,
        vec![assignment]
    );
    assert_eq!(snapshot.high_water().position.get(), 2);
    let feed = snapshot
        .feed_after(
            FeedCursor {
                epoch,
                position: FeedPosition::ZERO,
            },
            10,
            false,
        )
        .await?;
    assert!(
        matches!(feed, FeedRead::Entries { entries, covered_through, .. } if entries.len() == 2 && covered_through.position.get() == 2)
    );
    snapshot.finish().await?;
    migrated.close().await;
    Ok(())
}
