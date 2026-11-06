//! Immutable first decisions and full refusal trees have typed owner-approved columns.
use super::workspace_columns_tests::prepare;
use crate::fleet_storage_fixture::TestError;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The approved first-decision, reason tree and ordered evidence fields must replace the payload column."
)]
async fn ledger_schema_exposes_typed_decisions_reason_nodes_and_evidence() -> Result<(), TestError>
{
    let fixture = prepare().await?;
    let mut reasons = vec![
        "reason_id",
        "request_id",
        "parent_reason_id",
        "ordinal",
        "refusal_group",
        "refusal_kind",
        "record_kind",
        "record_id",
        "name",
        "field",
        "current_revision",
        "retry_after_seconds",
        "workstream_id",
        "task_id",
        "thread_id",
        "requirement_kind",
        "requirement_role",
        "role_subject_kind",
        "role_subject_id",
        "authority_role",
    ];
    for fields in [
        [
            "actor_kind",
            "actor_human_id",
            "actor_service_id",
            "actor_endpoint_id",
            "actor_session_id",
        ],
        [
            "holder_kind",
            "holder_human_id",
            "holder_service_id",
            "holder_endpoint_id",
            "holder_session_id",
        ],
        [
            "target_kind",
            "target_human_id",
            "target_service_id",
            "target_endpoint_id",
            "target_session_id",
        ],
        [
            "named_holder_kind",
            "named_holder_human_id",
            "named_holder_service_id",
            "named_holder_endpoint_id",
            "named_holder_session_id",
        ],
    ] {
        reasons.extend(fields);
    }
    reasons.extend([
        "last_seen_epoch",
        "last_seen_position",
        "evidence_kind",
        "evidence_returned_count",
        "evidence_total_count",
        "evidence_digest",
        "evidence_abridged",
        "created_at",
        "updated_at",
    ]);
    for (table, expected) in [
        (
            "ledger",
            vec![
                "request_id",
                "canonical_hash",
                "workspace_id",
                "board_id",
                "actor_kind",
                "actor_human_id",
                "actor_service_id",
                "actor_endpoint_id",
                "actor_session_id",
                "actor_router_component",
                "envelope_request_id",
                "fleet_position",
                "router_time",
                "time_basis",
                "origin_installation",
                "origin_machine",
                "created_at",
                "updated_at",
                "decision_kind",
                "change_kind",
                "effects_count",
                "effects_digest",
                "reason_count",
            ],
        ),
        ("reasons", reasons),
        (
            "evidence",
            vec![
                "row_id",
                "reason_id",
                "ordinal",
                "entry_kind",
                "task_id",
                "assignment_kind",
                "assignment_project_id",
                "assignment_workstream_id",
                "created_at",
                "updated_at",
            ],
        ),
    ] {
        let columns: Vec<(String, String, i64)> = fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    let sql = match table {
                        "ledger" => "PRAGMA table_info(fleet_request_decisions)",
                        "reasons" => "PRAGMA table_info(fleet_decision_reasons)",
                        "evidence" => "PRAGMA table_info(fleet_decision_reason_evidence)",
                        _ => return Err(StoreError::backend("unknown ledger fixture table")),
                    };
                    query::<sqlx_turso::Turso>(sql)
                        .fetch_all(scope.executor())
                        .await
                        .map_err(StoreError::backend)?
                        .into_iter()
                        .map(|row| {
                            Ok((
                                row.try_get("name").map_err(StoreError::backend)?,
                                row.try_get("type").map_err(StoreError::backend)?,
                                row.try_get("pk").map_err(StoreError::backend)?,
                            ))
                        })
                        .collect()
                })
            })
            .await?;
        assert_eq!(columns.len(), expected.len(), "{table}: {columns:?}");
        for (index, name) in expected.into_iter().enumerate() {
            let storage = if [
                "fleet_position",
                "created_at",
                "updated_at",
                "reason_count",
                "ordinal",
                "evidence_returned_count",
                "evidence_abridged",
            ]
            .contains(&name)
            {
                "INTEGER"
            } else {
                "TEXT"
            };
            assert!(
                columns.contains(&(name.into(), storage.into(), i64::from(index == 0))),
                "{table}.{name}: {columns:?}"
            );
        }
    }
    fixture.close().await?;
    Ok(())
}

pub(super) fn identity(session: bool) -> Result<message_board::Identity, TestError> {
    Ok(if session {
        message_board::Identity::Session {
            session: message_board::SessionRef {
                endpoint: message_board::SessionEndpointRef {
                    service_id: "550e8400-e29b-41d4-a716-4466554400a9"
                        .to_owned()
                        .try_into()?,
                    endpoint_id: "codex-local".to_owned().try_into()?,
                },
                session_id: "opaque historical λ".to_owned().try_into()?,
            },
        }
    } else {
        message_board::Identity::Human {
            human_id: "literal human".to_owned().try_into()?,
        }
    })
}
pub(super) fn refusals() -> Result<Vec<message_board::Refusal>, TestError> {
    use message_board::*;
    let record = RecordRef {
        kind: RecordKind::Post,
        id: "019f0000-0000-7000-8000-00000000ac01"
            .to_owned()
            .try_into()?,
    };
    let task: TaskId = "019f0000-0000-7000-8000-00000000ac02"
        .to_owned()
        .try_into()?;
    let workstream: WorkstreamId = "019f0000-0000-7000-8000-00000000ac03"
        .to_owned()
        .try_into()?;
    let project: ProjectId = "019f0000-0000-7000-8000-00000000ac04"
        .to_owned()
        .try_into()?;
    let thread: ThreadId = "019f0000-0000-7000-8000-00000000ac05"
        .to_owned()
        .try_into()?;
    let cursor = FeedCursor {
        epoch: EpochNumber::try_from(i64::MAX as u64)?,
        position: FeedPosition::try_from(i64::MAX as u64)?,
    };
    let assignments = vec![
        Assignment::Lead(project.clone()),
        Assignment::Advisor(project.clone()),
        Assignment::Implementer(workstream.clone()),
        Assignment::Verifier(workstream.clone()),
        Assignment::Reviewer(workstream.clone()),
        Assignment::Operator(workstream.clone()),
    ];
    let mut values = vec![
        Refusal::Archived,
        Refusal::InvalidShape {
            field: "literal.field".into(),
        },
        Refusal::StaleRevision {
            current: SubjectRevision::try_from(i64::MAX as u64)?,
        },
        Refusal::NameConflict {
            name: "literal name".to_owned().try_into()?,
            existing: record.clone(),
        },
        Refusal::AlreadyExists(record.clone()),
        Refusal::Closed {
            subject: record.clone(),
        },
        Refusal::TopLevelPostCooldown {
            retry_after_seconds: PostCooldownSeconds::try_from(60)?,
        },
        Refusal::Workstream(WorkstreamRefusal::PairNotDistinct {
            holder: identity(true)?,
        }),
        Refusal::Workstream(WorkstreamRefusal::Closed {
            workstream: workstream.clone(),
        }),
        Refusal::Workstream(WorkstreamRefusal::NotFound {
            record: record.clone(),
        }),
        Refusal::Task(TaskRefusal::DependencyCycle {
            path: AbridgedEvidence::from_full(vec![task.clone(), task.clone()]),
        }),
        Refusal::Task(TaskRefusal::DependencyCycle {
            path: AbridgedEvidence::from_parts(
                vec![task.clone(); 256],
                u64::MAX,
                "ab".repeat(32).try_into()?,
                true,
            )?,
        }),
        Refusal::Task(TaskRefusal::DependencyMissing(task.clone())),
        Refusal::Task(TaskRefusal::DependencyDropped(task.clone())),
        Refusal::Task(TaskRefusal::DependencyCrossesProject(task)),
        Refusal::Task(TaskRefusal::NotFound {
            record: record.clone(),
        }),
        Refusal::Task(TaskRefusal::Closed {
            subject: record.clone(),
        }),
        Refusal::Task(TaskRefusal::StaleRevision {
            current: SubjectRevision::INITIAL,
        }),
        Refusal::Trail(TrailRefusal::SubjectRequired {
            assignments: AbridgedEvidence::from_full(assignments.clone()),
        }),
        Refusal::Trail(TrailRefusal::SubjectRequired {
            assignments: AbridgedEvidence::from_parts(
                assignments.iter().cloned().cycle().take(256).collect(),
                u64::MAX,
                "cd".repeat(32).try_into()?,
                true,
            )?,
        }),
        Refusal::Trail(TrailRefusal::NotFound {
            record: record.clone(),
        }),
        Refusal::Trail(TrailRefusal::InvalidShape {
            field: "λ".repeat(128),
        }),
        Refusal::Role(RoleRefusal::PairNotDistinct {
            holder: identity(false)?,
        }),
        Refusal::Role(RoleRefusal::StandDownUnacknowledged {}),
        Refusal::Thread(Box::new(ThreadRefusal::ParticipantRequired {
            thread_id: thread.clone(),
            actor: identity(true)?,
        })),
        Refusal::Thread(Box::new(ThreadRefusal::OrchestratorAlreadyExists {
            thread_id: thread.clone(),
            actor: identity(true)?,
            holder: identity(false)?,
            last_seen: cursor,
        })),
        Refusal::Thread(Box::new(ThreadRefusal::ImplementerAlreadyExists {
            thread_id: thread.clone(),
            actor: identity(false)?,
            holder: identity(true)?,
            last_seen: cursor,
        })),
        Refusal::Thread(Box::new(ThreadRefusal::OrchestratorHandoverRequired {
            thread_id: thread.clone(),
            actor: identity(false)?,
        })),
        Refusal::Thread(Box::new(ThreadRefusal::HandoverTargetNotParticipant {
            thread_id: thread.clone(),
            actor: identity(false)?,
            target: identity(true)?,
        })),
        Refusal::Thread(Box::new(ThreadRefusal::SelfReplace {
            thread_id: thread.clone(),
            actor: identity(true)?,
        })),
        Refusal::Thread(Box::new(ThreadRefusal::OrchestratorRoleChange {
            thread_id: thread.clone(),
            actor: identity(false)?,
        })),
    ];
    for role in [
        AuthorityRole::Lead,
        AuthorityRole::Advisor,
        AuthorityRole::Implementer,
        AuthorityRole::Verifier,
        AuthorityRole::Reviewer,
        AuthorityRole::Operator,
    ] {
        for subject in [
            RoleSubject::Project(project.clone()),
            RoleSubject::Workstream(workstream.clone()),
        ] {
            values.push(Refusal::Role(RoleRefusal::NoHolder { subject, role }));
        }
        values.push(Refusal::Workstream(WorkstreamRefusal::RoleRequired {
            requirement: RoleRequirement::Holds(role),
        }));
        values.push(Refusal::Role(RoleRefusal::RoleRequired {
            requirement: RoleRequirement::Holds(role),
        }));
    }
    for requirement in [
        RoleRequirement::LeadOrWorkstreamImplementer,
        RoleRequirement::AnyProjectRole,
    ] {
        values.push(Refusal::Workstream(WorkstreamRefusal::RoleRequired {
            requirement: requirement.clone(),
        }));
        values.push(Refusal::Role(RoleRefusal::RoleRequired { requirement }));
    }
    for holder in [None, Some(identity(true)?)] {
        for last_seen in [None, Some(cursor)] {
            values.push(Refusal::Thread(Box::new(
                ThreadRefusal::OrchestratorRequired {
                    thread_id: thread.clone(),
                    actor: identity(false)?,
                    holder: holder.clone(),
                    last_seen,
                },
            )));
            if holder.is_some() == last_seen.is_some() {
                values.push(Refusal::Thread(Box::new(
                    ThreadRefusal::StaleOrchestrator {
                        thread_id: thread.clone(),
                        actor: identity(true)?,
                        holder: holder.clone(),
                        last_seen,
                        named_holder: identity(false)?,
                    },
                )));
                values.push(Refusal::Thread(Box::new(ThreadRefusal::StaleImplementer {
                    thread_id: thread.clone(),
                    actor: identity(false)?,
                    holder: holder.clone(),
                    last_seen,
                    named_holder: identity(true)?,
                })));
            }
        }
    }
    for kind in [
        RecordKind::Workspace,
        RecordKind::Board,
        RecordKind::Adr,
        RecordKind::Project,
        RecordKind::Workstream,
        RecordKind::Task,
        RecordKind::TaskLinkEvent,
        RecordKind::Subtask,
        RecordKind::Thread,
        RecordKind::ThreadRefiled,
        RecordKind::Post,
        RecordKind::RoleAssignment,
        RecordKind::OwnerGrant,
        RecordKind::StandDownAcknowledgement,
        RecordKind::Fact,
        RecordKind::PlanRevision,
        RecordKind::ReadMark,
        RecordKind::NotifyIntent,
        RecordKind::SubscriptionMembership,
        RecordKind::Request,
        RecordKind::WorkTrailRow,
        RecordKind::TrailRedaction,
    ] {
        values.push(Refusal::NotFound(RecordRef {
            kind,
            id: record.id.clone(),
        }));
    }
    let wrapped = values
        .iter()
        .cloned()
        .map(|reason| Refusal::ProjectRejected {
            reason: Box::new(reason),
        })
        .collect::<Vec<_>>();
    values.extend(wrapped);
    Ok(values)
}

pub(crate) fn entry(
    index: usize,
    decision: message_board::StoredDecision,
) -> Result<message_board::RequestLedgerEntry, TestError> {
    use message_board::*;
    let Record::Workspace(workspace) = super::workspace_columns_tests::workspace(1)? else {
        return Err("wrong fixture".into());
    };
    let id: RequestId = format!("019f0000-0000-7000-8000-{index:012x}").try_into()?;
    let mut envelope = workspace.envelope;
    envelope.id = id.clone().into();
    envelope.time = RecordTime::Router("2026-11-04T10:11:12.123456789Z".parse()?);
    envelope.scope.board = Some(
        "019f0000-0000-7000-8000-00000000ac08"
            .to_owned()
            .try_into()?,
    );
    envelope.actor = if index.is_multiple_of(3) {
        RecordedActor::Router {
            component: RouterComponent::ArchiveObservation,
            installation: "550e8400-e29b-41d4-a716-4466554400a9"
                .to_owned()
                .try_into()?,
        }
    } else if index % 3 == 1 {
        RecordedActor::Session {
            session: SessionRef {
                endpoint: SessionEndpointRef {
                    service_id: "550e8400-e29b-41d4-a716-4466554400a9"
                        .to_owned()
                        .try_into()?,
                    endpoint_id: "codex-local".to_owned().try_into()?,
                },
                session_id: "literal requester".to_owned().try_into()?,
            },
        }
    } else {
        RecordedActor::Human {
            human_id: "literal requester".to_owned().try_into()?,
        }
    };
    Ok(RequestLedgerEntry {
        envelope,
        request_id: id,
        canonical_hash: "ab".repeat(32).try_into()?,
        accepted_in: None,
        decision,
    })
}
pub(crate) async fn write(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
    entry: message_board::RequestLedgerEntry,
    at: i64,
) -> Result<fleet_admission::FleetRequestResult, StoreError> {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                crate::fleet_decision_step::persist_decision(
                    scope,
                    crate::fleet_decision_step::FleetDecisionStep {
                        entry,
                        effects: Vec::new(),
                        receipt_ids: Vec::new(),
                        now: runtime_primitives::SyncTime::from_unix_millis(at),
                        row_ids: crate::fleet_row_ids::FleetRowIds::new(std::sync::Arc::new(
                            crate::fleet_storage_fixture::FixtureIo,
                        )),
                        #[cfg(feature = "conformance")]
                        fault: crate::fleet_decision_step::FleetDecisionFault::None,
                    },
                )
                .await
            })
        })
        .await
}
pub(super) async fn read(
    fixture: &crate::fleet_storage_fixture::FleetStorageFixture,
    id: message_board::RequestId,
) -> Result<Option<message_board::RequestLedgerEntry>, StoreError> {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move { crate::fleet_decision_step::read_ledger(scope, &id).await })
        })
        .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Every declared refusal family, optional identity/seat shape, cursor and complete/abridged commitment must survive its real typed owning store."
)]
async fn ledger_preserves_full_shared_refusal_vocabulary_and_commitments_across_owning_reopen()
-> Result<(), TestError> {
    use message_board::StoredDecision;
    let fixture = prepare().await?;
    let mut expected = Vec::new();
    for (index, reason) in refusals()?.into_iter().enumerate() {
        let value = entry(
            index,
            StoredDecision::Rejected {
                reasons: vec![reason],
            },
        )?;
        let reply = write(
            &fixture,
            value.clone(),
            crate::fleet_storage_fixture::NOW.unix_millis(),
        )
        .await?;
        assert!(matches!(
            reply,
            fleet_admission::FleetRequestResult::Decided(
                message_board::RequestDecision::Rejected { .. }
            )
        ));
        assert_eq!(
            read(&fixture, value.request_id.clone()).await?,
            Some(value.clone())
        );
        expected.push(value);
    }
    let wrong_times = fixture.object.transaction(|scope| Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT count(*) AS wrong FROM (SELECT created_at,updated_at FROM fleet_request_decisions UNION ALL SELECT created_at,updated_at FROM fleet_decision_reasons UNION ALL SELECT created_at,updated_at FROM fleet_decision_reason_evidence) WHERE created_at<>?1 OR updated_at<>?1")
            .bind(crate::fleet_storage_fixture::NOW.unix_millis()).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        row.try_get::<i64,_>("wrong").map_err(StoreError::backend)
    })).await?;
    assert_eq!(
        wrong_times, 0,
        "all typed decision children use the existing injected owner clock"
    );
    let fixture = fixture.reopen().await?;
    let ids = expected
        .iter()
        .map(|entry| entry.request_id.clone())
        .collect::<Vec<_>>();
    let loaded = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move { crate::fleet_decision_step::read_ledgers(scope, &ids).await })
        })
        .await?;
    assert_eq!(loaded.len(), expected.len());
    for value in expected {
        assert_eq!(loaded.get(&value.request_id), Some(&value));
    }
    fixture.close().await?;
    Ok(())
}
