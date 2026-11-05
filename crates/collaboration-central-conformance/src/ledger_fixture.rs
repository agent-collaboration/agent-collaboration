//! Literal typed-ledger cases shared by the native and real-worker store probes.
use crate::{ConformanceResult, receipt_fixture};
use message_board::{
    AbridgedEvidence, Assignment, AuthorityRole, EpochNumber, FeedCursor, FeedPosition, Identity,
    ProjectId, RecordTime, Refusal, RequestLedgerEntry, RoleRefusal, RoleRequirement, RoleSubject,
    SessionEndpointRef, SessionRef, StoredDecision, SubjectRevision, TaskId, TaskRefusal, ThreadId,
    ThreadRefusal, TrailRefusal, WorkstreamId, WorkstreamRefusal,
};

pub fn typed_ledger_fixtures() -> ConformanceResult<Vec<RequestLedgerEntry>> {
    let task: TaskId = "019f0000-0000-7000-8000-00000000ae01"
        .to_owned()
        .try_into()?;
    let project: ProjectId = "019f0000-0000-7000-8000-00000000ae02"
        .to_owned()
        .try_into()?;
    let workstream: WorkstreamId = "019f0000-0000-7000-8000-00000000ae03"
        .to_owned()
        .try_into()?;
    let thread: ThreadId = "019f0000-0000-7000-8000-00000000ae04"
        .to_owned()
        .try_into()?;
    let actor = Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: "550e8400-e29b-41d4-a716-4466554400a9"
                    .to_owned()
                    .try_into()?,
                endpoint_id: "codex-local".to_owned().try_into()?,
            },
            session_id: "literal ledger requester λ".to_owned().try_into()?,
        },
    };
    let holder = Identity::Human {
        human_id: "literal holder".to_owned().try_into()?,
    };
    let assignments = vec![
        Assignment::Lead(project.clone()),
        Assignment::Advisor(project.clone()),
        Assignment::Implementer(workstream.clone()),
        Assignment::Verifier(workstream.clone()),
        Assignment::Reviewer(workstream.clone()),
        Assignment::Operator(workstream),
    ];
    let mut reasons = vec![
        Refusal::Archived,
        Refusal::StaleRevision {
            current: SubjectRevision::try_from(i64::MAX as u64)?,
        },
        Refusal::Task(TaskRefusal::DependencyCycle {
            path: AbridgedEvidence::from_full(vec![task.clone(), task.clone()]),
        }),
        Refusal::Task(TaskRefusal::DependencyCycle {
            path: AbridgedEvidence::from_parts(
                vec![task; 256],
                u64::MAX,
                "ab".repeat(32).try_into()?,
                true,
            )?,
        }),
        Refusal::Trail(TrailRefusal::SubjectRequired {
            assignments: AbridgedEvidence::from_full(assignments.clone()),
        }),
        Refusal::Trail(TrailRefusal::SubjectRequired {
            assignments: AbridgedEvidence::from_parts(
                assignments.into_iter().cycle().take(256).collect(),
                u64::MAX,
                "cd".repeat(32).try_into()?,
                true,
            )?,
        }),
        Refusal::Role(RoleRefusal::NoHolder {
            subject: RoleSubject::Project(project),
            role: AuthorityRole::Advisor,
        }),
        Refusal::Workstream(WorkstreamRefusal::RoleRequired {
            requirement: RoleRequirement::LeadOrWorkstreamImplementer,
        }),
    ];
    for cursor in [
        None,
        Some(FeedCursor {
            epoch: EpochNumber::try_from(i64::MAX as u64)?,
            position: FeedPosition::try_from(i64::MAX as u64)?,
        }),
    ] {
        reasons.push(Refusal::Thread(Box::new(
            ThreadRefusal::OrchestratorRequired {
                thread_id: thread.clone(),
                actor: actor.clone(),
                holder: Some(holder.clone()),
                last_seen: cursor,
            },
        )));
        reasons.push(Refusal::Thread(Box::new(ThreadRefusal::StaleImplementer {
            thread_id: thread.clone(),
            actor: actor.clone(),
            holder: cursor.map(|_| holder.clone()),
            last_seen: cursor,
            named_holder: holder.clone(),
        })));
    }
    let wrapped = reasons
        .iter()
        .cloned()
        .map(|reason| Refusal::ProjectRejected {
            reason: Box::new(reason),
        })
        .collect::<Vec<_>>();
    reasons.extend(wrapped);
    reasons
        .into_iter()
        .enumerate()
        .map(|(index, reason)| {
            let mut entry = receipt_fixture(
                format!("019f0000-0000-7000-8000-00000000af{index:02x}").try_into()?,
            )?;
            entry.envelope.request = Some(
                "019f0000-0000-7000-8000-00000000ae05"
                    .to_owned()
                    .try_into()?,
            );
            entry.envelope.time = RecordTime::Router("2026-11-04T10:11:12.123456789Z".parse()?);
            entry.decision = StoredDecision::Rejected {
                reasons: vec![reason],
            };
            Ok(entry)
        })
        .collect()
}
