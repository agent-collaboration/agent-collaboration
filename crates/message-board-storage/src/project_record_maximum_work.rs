//! Finite work and authority fields use their full byte limits in native rows.
use super::{
    RECORD_BYTE_BUDGET, SCHEMA_BYTE_BUDGET, TestResult, envelope, maximum_actor,
    measure_native_row, project_state,
};
use message_board::*;

fn maximum_workstream(state: &ProjectState) -> TestResult<Record> {
    let id = WorkstreamId::generate();
    Ok(Record::Workstream(Workstream {
        envelope: envelope(state, id.clone().into()),
        workstream_id: id,
        project_id: state.project_id.clone(),
        name: ResourceName::try_from("\"".repeat(MAX_NAME_BYTES))?,
        goal: "\0".repeat(MAX_MESSAGE_TEXT_BYTES),
        state: WorkstreamState::Closed {
            reason: CloseReason::Dropped,
            note: "\0".repeat(MAX_MESSAGE_TEXT_BYTES),
        },
        revision: SubjectRevision::try_from(i64::MAX as u64)?,
    }))
}

fn maximum_task(state: &ProjectState) -> TestResult<Record> {
    let id = TaskId::generate();
    Ok(Record::Task(Task {
        envelope: envelope(state, id.clone().into()),
        task_id: id,
        project_id: state.project_id.clone(),
        workstream_id: WorkstreamId::generate(),
        title: TaskTitle::try_from("\"".repeat(MAX_NAME_BYTES))?,
        body: "\0".repeat(MAX_MESSAGE_TEXT_BYTES),
        status: TaskStatus::Dropped,
        revision: SubjectRevision::try_from(i64::MAX as u64)?,
    }))
}

fn maximum_plan_revision(state: &ProjectState) -> TestResult<Record> {
    Ok(Record::PlanRevision(PlanRevision {
        envelope: envelope(state, RecordId::generate()),
        hash: PlanRevisionHash::try_from("f".repeat(64))?,
        document: EvidencePointer::RepoPath {
            repository: NormalizedOrigin::try_from(format!(
                "a.b/{}",
                "\u{0001}".repeat(MAX_REPOSITORY_LOCATOR_BYTES - 4)
            ))?,
            path: "\0".repeat(MAX_EVIDENCE_PATH_BYTES),
            commit: Some(CommitSha::try_from("f".repeat(64))?),
        },
        superseded_by: Some(PlanRevisionHash::try_from("e".repeat(64))?),
        revision: SubjectRevision::try_from(i64::MAX as u64)?,
    }))
}

fn maximum_owner_grant(state: &ProjectState) -> TestResult<Record> {
    let id = OwnerGrantId::generate();
    Ok(Record::OwnerGrant(OwnerGrant {
        envelope: envelope(state, id.clone().into()),
        id,
        grant: GrantKind::ReplaceLead {
            project: state.project_id.clone(),
            new_holder: maximum_actor()?,
            reason: "\0".repeat(MAX_MESSAGE_TEXT_BYTES),
        },
        provenance: OwnerWordsProvenance::Relayed {
            by: maximum_actor()?,
        },
    }))
}

fn maximum_stood_down_assignment(state: &ProjectState) -> TestResult<Record> {
    let id = RoleAssignmentId::generate();
    Ok(Record::RoleAssignment(RoleAssignment::new(
        RoleAssignmentProps {
            envelope: envelope(state, id.clone().into()),
            assignment_id: id,
            subject: RoleSubject::Project(state.project_id.clone()),
            role: AuthorityRole::Lead,
            holder: maximum_actor()?,
            granted_by: maximum_actor()?,
            model: Some(ResourceName::try_from("\"".repeat(MAX_NAME_BYTES))?),
            lineage: Some(ResourceName::try_from("\"".repeat(MAX_NAME_BYTES))?),
            ended: Some(RoleEnd::StoodDown {
                reason: "\0".repeat(MAX_MESSAGE_TEXT_BYTES),
            }),
            owner_grant: Some(OwnerGrantId::generate()),
        },
    )?))
}

fn maximum_workstream_assignment(state: &ProjectState) -> TestResult<Record> {
    let Record::RoleAssignment(assignment) = maximum_stood_down_assignment(state)? else {
        return Err("assignment fixture".into());
    };
    let mut props: RoleAssignmentProps = assignment.into();
    props.subject = RoleSubject::Workstream(WorkstreamId::generate());
    props.role = AuthorityRole::Implementer;
    Ok(Record::RoleAssignment(RoleAssignment::new(props)?))
}

fn maximum_subtask(state: &ProjectState) -> TestResult<Record> {
    let id = SubtaskId::generate();
    Ok(Record::Subtask(Subtask {
        envelope: envelope(state, id.clone().into()),
        subtask_id: id,
        task_id: TaskId::generate(),
        text: TrailText::try_from("\"".repeat(MAX_MESSAGE_TEXT_BYTES))?,
        checked: false,
        removed: false,
        place: SubtaskPlace::try_from("f".repeat(MAX_ORDER_KEY_HEX_BYTES))?,
    }))
}

fn maximum_task_link_event(state: &ProjectState) -> Record {
    let id = TaskLinkEventId::generate();
    Record::TaskLinkEvent(TaskLinkEvent {
        envelope: envelope(state, id.clone().into()),
        id,
        change: TaskLinkEventKind::Removed(TaskLink {
            from: TaskId::generate(),
            kind: TaskLinkKind::BlockedBy,
            to: TaskId::generate(),
        }),
    })
}

fn maximum_stand_down_acknowledgement(state: &ProjectState) -> TestResult<Record> {
    Ok(Record::StandDownAcknowledgement(StandDownAcknowledgement {
        envelope: envelope(state, RecordId::generate()),
        assignment: RoleAssignmentId::generate(),
        read_through: FeedCursor {
            epoch: EpochNumber::try_from(i64::MAX as u64)?,
            position: FeedPosition::try_from(i64::MAX as u64)?,
        },
    }))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn maximum_work_and_authority_rows_fit_native_storage_without_truncation() -> TestResult {
    // Inputs use the governing finite caller limits and maximum repeated actor
    // metadata. The shared helper checks exact current/feed round trips and
    // measures the native snapshot's complete SQL row, not only its record JSON.
    let state = project_state()?;
    let cases = [
        ("maximumClosedWorkstream", maximum_workstream(&state)?),
        ("maximumTask", maximum_task(&state)?),
        ("maximumPlanRevision", maximum_plan_revision(&state)?),
        ("maximumOwnerGrant", maximum_owner_grant(&state)?),
        (
            "maximumStoodDownAssignment",
            maximum_stood_down_assignment(&state)?,
        ),
        (
            "maximumWorkstreamAssignment",
            maximum_workstream_assignment(&state)?,
        ),
        ("maximumSubtask", maximum_subtask(&state)?),
        ("maximumTaskLinkEvent", maximum_task_link_event(&state)),
        (
            "maximumStandDownAcknowledgement",
            maximum_stand_down_acknowledgement(&state)?,
        ),
    ];
    let mut measured = Vec::new();
    for (kind, record) in cases {
        record.validate_bounds()?;
        let size = measure_native_row(kind, &state, record).await?;
        assert!(size.schema_statements > 0);
        assert!(
            size.conservative_numeric_row_bytes <= RECORD_BYTE_BUDGET,
            "{kind}: {} bytes",
            size.conservative_numeric_row_bytes
        );
        assert!(
            size.schema_bytes <= SCHEMA_BYTE_BUDGET,
            "{kind} schema: {} bytes",
            size.schema_bytes
        );
        measured.push(size);
    }
    eprintln!(
        "BOARD_MAXIMUM_WORK_SIZES {}",
        serde_json::to_string(&measured)?
    );
    Ok(())
}
