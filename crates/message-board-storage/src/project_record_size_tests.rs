//! Measure actual native feed rows and reconstructed schema before changing limits.
use crate::{
    ProjectStorageError, ProjectStore, project_store_tests::project_record,
    project_test_support::origin,
};
use fleet_sync_protocol::SqlValue;
use message_board::*;
use serde::Serialize;
#[path = "project_record_maximum_decisions.rs"]
mod maximum_decisions;
#[path = "project_record_maximum_evidence.rs"]
mod maximum_evidence;
#[path = "project_record_maximum_facts.rs"]
mod maximum_facts;
#[path = "project_record_maximum_messages.rs"]
mod maximum_messages;
#[path = "project_record_maximum_read_mark.rs"]
mod maximum_read_mark;
#[path = "project_record_maximum_work.rs"]
mod maximum_work;
#[path = "project_record_size_prerequisites.rs"]
mod prerequisites;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn oversized_trail_fields_are_refused_at_the_native_write_boundary() -> TestResult {
    let state = project_state()?;
    for field in ["tags", "links", "path", "url"] {
        let Record::WorkTrailRow(mut row) = maximum_trail(&state, 16)? else {
            panic!("trail fixture");
        };
        match field {
            "tags" => row.tags.push(TrailTag::try_from("extra".to_owned())?),
            "links" => row.links.push(row.links[0].clone()),
            "path" => {
                row.links[0] = EvidencePointer::RepoPath {
                    repository: NormalizedOrigin::try_from("example.org/repo".to_owned())?,
                    path: "x".repeat(1025),
                    commit: None,
                }
            }
            "url" => {
                row.links[0] = EvidencePointer::Url {
                    url: EvidenceUrl::try_from(format!(
                        "https://example.org/{}",
                        "x".repeat(2048)
                    ))?,
                }
            }
            _ => return Err("unknown oversized trail field fixture".into()),
        }
        let result = measure_native_row("oversizedTrail", &state, Record::WorkTrailRow(row)).await;
        let Err(error) = result else {
            panic!("an oversized trail field must not be admitted");
        };
        assert!(
            matches!(error.downcast_ref::<ProjectStorageError>(),
            Some(ProjectStorageError::InvalidRecord { field: actual, .. }) if *actual == field),
            "{field}: {error}"
        );
    }
    Ok(())
}

const RECORD_BYTE_BUDGET: usize = 1024 * 1024;
const SCHEMA_BYTE_BUDGET: usize = 256 * 1024;
const TRAIL_TAGS: usize = 32;
const TRAIL_LINKS: usize = 16;
const REPOSITORY_PATH_BYTES: usize = 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeRowSize {
    kind: &'static str,
    feed_row_bytes: usize,
    conservative_numeric_row_bytes: usize,
    schema_bytes: usize,
    schema_statements: usize,
}

fn maximum_integer_width(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Number(number) if number.is_i64() || number.is_u64() => {
            // Twenty bytes covers every signed or unsigned 64-bit integer.
            *number = serde_json::Number::from(i64::MIN);
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(maximum_integer_width),
        serde_json::Value::Object(fields) => fields.values_mut().for_each(maximum_integer_width),
        _ => {}
    }
}

fn maximum_numeric_width_row(row: &[SqlValue]) -> TestResult<usize> {
    // Keep the native row and exact round-trip oracle intact. This separate
    // upper bound permits every assigned integer to use its largest encoded
    // width, without pretending the fixture executed an enormous history.
    let bounded = row
        .iter()
        .map(|cell| -> TestResult<SqlValue> {
            match cell {
                SqlValue::Integer(_) => Ok(SqlValue::Integer(i64::MIN)),
                SqlValue::Text(text) => {
                    let Ok(mut content) = serde_json::from_str::<serde_json::Value>(text) else {
                        return Ok(cell.clone());
                    };
                    if content.is_object() || content.is_array() {
                        maximum_integer_width(&mut content);
                        Ok(SqlValue::Text(serde_json::to_string(&content)?))
                    } else {
                        Ok(cell.clone())
                    }
                }
                _ => Ok(cell.clone()),
            }
        })
        .collect::<TestResult<Vec<_>>>()?;
    let original_bytes = serde_json::to_vec(row)?.len();
    let upper_bytes = serde_json::to_vec(&bounded)?.len();
    assert!(
        upper_bytes >= original_bytes,
        "numeric reservation reduced the real row size"
    );
    Ok(upper_bytes)
}

fn maximum_actor() -> TestResult<RecordedActor> {
    Ok(RecordedActor::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
                endpoint_id: EndpointId::try_from("a".repeat(64))?,
            },
            session_id: SessionId::try_from("\u{0001}".repeat(4096))?,
        },
    })
}

fn project_state() -> TestResult<ProjectState> {
    let Record::Project(mut state) = project_record(ProjectId::generate()) else {
        return Err("project fixture changed its record kind".into());
    };
    state.envelope.actor = maximum_actor()?;
    state.envelope.request = Some(RequestId::generate());
    state.envelope.time = RecordTime::Router(chrono::DateTime::<chrono::Utc>::MAX_UTC);
    Ok(state)
}

fn envelope(state: &ProjectState, id: RecordId) -> RecordEnvelope {
    let mut envelope = state.envelope.clone();
    envelope.id = id;
    envelope
}

fn maximum_post(state: &ProjectState) -> TestResult<Record> {
    let post_id = PostId::generate();
    let actor = maximum_actor()?
        .declared_identity()
        .ok_or("declared actor")?;
    Ok(Record::Post(Post {
        envelope: envelope(state, post_id.clone().into()),
        post_id,
        board_id: state.envelope.scope.board.clone().ok_or("board scope")?,
        project_id: state.project_id.clone(),
        placement: Placement::Thread {
            thread_id: ThreadId::generate(),
        },
        actor,
        acting_for: Some(ActingForIdentity::Human {
            human_id: HumanId::try_from("\u{0001}".repeat(4096))?,
        }),
        text: PostText::try_from("\"".repeat(MAX_MESSAGE_TEXT_BYTES))?,
        references: (0..MAX_MESSAGE_REFERENCES)
            .map(|_| ReferenceTarget::Thread {
                thread_id: ThreadId::generate(),
            })
            .collect::<Vec<_>>()
            .try_into()?,
        position: state.envelope.position.clone(),
        posted_as_role: Some(ParticipantRole::Orchestrator),
    }))
}

fn maximum_trail(state: &ProjectState, links: usize) -> TestResult<Record> {
    let row_id = WorkTrailRowId::generate();
    // NormalizedOrigin accepts this canonical repository path. Its C0 bytes
    // exercise the actual maximum escaping, rather than a short URL example.
    let repository = NormalizedOrigin::try_from(format!(
        "a.b/{}",
        "\u{0001}".repeat(MAX_REPOSITORY_LOCATOR_BYTES - 4)
    ))?;
    let tag = TrailTag::try_from("\"".repeat(MAX_NAME_BYTES))?;
    Ok(Record::WorkTrailRow(WorkTrailRow {
        envelope: envelope(state, row_id.clone().into()),
        row_id,
        project_id: state.project_id.clone(),
        subject: TrailSubject::Workstream(WorkstreamId::generate()),
        text: TrailText::try_from("\"".repeat(MAX_MESSAGE_TEXT_BYTES))?,
        tags: vec![tag; TRAIL_TAGS],
        links: vec![
            EvidencePointer::RepoPath {
                repository,
                path: "\0".repeat(REPOSITORY_PATH_BYTES),
                commit: Some(CommitSha::try_from("f".repeat(64))?),
            };
            links
        ],
        redacted: false,
    }))
}

fn ledger(state: &ProjectState, decision: StoredDecision) -> TestResult<Record> {
    let request_id = RequestId::generate();
    let accepted_in =
        matches!(decision, StoredDecision::Accepted { .. }).then_some(EpochNumber::INITIAL);
    let mut envelope = envelope(state, request_id.clone().into());
    envelope.request = Some(request_id.clone());
    Ok(Record::Request(RequestLedgerEntry {
        envelope,
        request_id,
        canonical_hash: CanonicalHash::try_from("a".repeat(64))?,
        accepted_in,
        decision,
    }))
}

fn dependency_cycle(state: &ProjectState, items: usize) -> TestResult<Record> {
    let ids = (0..items).map(|_| TaskId::generate()).collect::<Vec<_>>();
    let first = ids.first().ok_or("cycle requires nodes")?;
    let second = ids.get(1).ok_or("cycle requires two nodes")?;
    let last = ids.last().ok_or("cycle requires nodes")?;
    let workstream_id = WorkstreamId::generate();
    let tasks = ids
        .iter()
        .map(|id| {
            Ok(Task {
                envelope: envelope(state, id.clone().into()),
                task_id: id.clone(),
                project_id: state.project_id.clone(),
                workstream_id: workstream_id.clone(),
                title: TaskTitle::try_from("cycle node".to_owned())?,
                body: String::new(),
                status: TaskStatus::Todo,
                revision: SubjectRevision::INITIAL,
            })
        })
        .collect::<TestResult<Vec<_>>>()?;
    let mut existing = ids
        .iter()
        .skip(1)
        .zip(ids.iter().skip(2))
        .map(|(from, to)| TaskLink {
            from: from.clone(),
            kind: TaskLinkKind::BlockedBy,
            to: to.clone(),
        })
        .collect::<Vec<_>>();
    existing.push(TaskLink {
        from: last.clone(),
        kind: TaskLinkKind::BlockedBy,
        to: first.clone(),
    });
    let reason = work_rules::add_task_link(
        &TaskLink {
            from: first.clone(),
            kind: TaskLinkKind::BlockedBy,
            to: second.clone(),
        },
        &tasks,
        &existing,
    )
    .err()
    .ok_or("the real dependency rule did not find its closed cycle")?;
    ledger(
        state,
        StoredDecision::Rejected {
            reasons: vec![Refusal::Task(reason)],
        },
    )
}

fn subject_assignments(state: &ProjectState, items: usize) -> TestResult<Record> {
    let assignments = (0..items)
        .map(|_| Assignment::Implementer(WorkstreamId::generate()))
        .collect::<Vec<_>>();
    let reason = work_rules::resolve_trail_subject(None, &assignments)
        .err()
        .ok_or("the real subject rule did not require an explicit subject")?;
    ledger(
        state,
        StoredDecision::Rejected {
            reasons: vec![Refusal::Trail(reason)],
        },
    )
}

fn dense_subtask_key(state: &ProjectState, inserts: usize) -> TestResult<Record> {
    let mut upper = None;
    for _ in 0..inserts {
        upper = Some(SubtaskPlace::between(None, upper.as_ref())?);
    }
    let place = upper.ok_or("key generation requires at least one insertion")?;
    let subtask_id = SubtaskId::generate();
    Ok(Record::Subtask(Subtask {
        envelope: envelope(state, subtask_id.clone().into()),
        subtask_id,
        task_id: TaskId::generate(),
        text: TrailText::try_from("dense insertion fixture".to_owned())?,
        checked: false,
        removed: false,
        place,
    }))
}

async fn measure_native_row(
    kind: &'static str,
    state: &ProjectState,
    record: Record,
) -> TestResult<NativeRowSize> {
    measure_native_row_with_effects(kind, state, record, 0).await
}

async fn measure_native_row_with_effects(
    kind: &'static str,
    state: &ProjectState,
    mut record: Record,
    prerequisite_effects: usize,
) -> TestResult<NativeRowSize> {
    let directory = tempfile::tempdir()?;
    let store = ProjectStore::open(
        &directory.path().join("project.sqlite"),
        state.project_id.clone(),
    )
    .await?;
    let reference = record.reference();
    let mut maximum_origin = origin();
    maximum_origin.machine = MachineLabel::try_from("\"".repeat(256))?;
    let mut write = store.begin_admission(maximum_origin).await?;
    if !matches!(record, Record::Project(_)) {
        write.append_record(Record::Project(state.clone())).await?;
    }
    prerequisites::append(&mut write, state, &mut record).await?;
    if prerequisite_effects > 0 {
        let Record::Request(ledger) = &mut record else {
            panic!("receipt fixture")
        };
        let mut effects = Vec::with_capacity(prerequisite_effects);
        for _ in 0..prerequisite_effects {
            let mut version = state.clone();
            version.envelope.request = Some(ledger.request_id.clone());
            let cursor = write.append_record(Record::Project(version)).await?;
            effects.push((
                state.project_id.clone().into(),
                write.record_placement(cursor),
            ));
        }
        ledger.decision = StoredDecision::accepted(effects, ChangeOutcome::Changed);
    }
    let mut expected = record.clone();
    let cursor = write.append_record(record).await?;
    let position = write.record_placement(cursor);
    expected.envelope_mut().position = position.clone();
    if let Record::Post(post) = &mut expected {
        post.position = position;
    }
    write.commit().await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.record(&reference).await?,
        Some(expected.clone()),
        "the native current row changed or truncated the supplied values"
    );
    read.finish().await?;
    let mut snapshot = store.database.snapshot().await?;
    let mut cursor = None;
    let mut measured = None;
    let mut numeric_upper_bound = None;
    let mut reconstructed = None;
    loop {
        let page = snapshot.page(cursor).await?;
        if let Some(previous) = &reconstructed {
            assert_eq!(previous, &page.schema_steps);
        } else {
            reconstructed = Some(page.schema_steps.clone());
        }
        if page.table == "project_feed" {
            for row in &page.rows {
                if !row.iter().any(
                    |value| matches!(value, SqlValue::Text(text) if text == reference.id.as_str()),
                ) {
                    continue;
                }
                let content = row.iter().find_map(|value| match value {
                    SqlValue::Text(text) => serde_json::from_str::<FeedContent>(text).ok(),
                    _ => None,
                });
                if !matches!(&content, Some(FeedContent::Shown(record))
                    if record.reference() == reference
                        && record.envelope().position == expected.envelope().position)
                {
                    continue;
                }
                assert!(
                    measured.is_none(),
                    "the case has more than one feed version"
                );
                let encoded = serde_json::to_vec(row)?;
                let decoded: Vec<SqlValue> = serde_json::from_slice(&encoded)?;
                assert_eq!(&decoded, row, "native row codec changed its values");
                assert!(
                    content == Some(FeedContent::Shown(Box::new(expected.clone()))),
                    "the captured history changed or truncated the supplied values"
                );
                numeric_upper_bound = Some(maximum_numeric_width_row(row)?);
                measured = Some(encoded.len());
            }
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    let schema = reconstructed.ok_or("snapshot had no reconstructed schema")?;
    let result = NativeRowSize {
        kind,
        feed_row_bytes: measured.ok_or("native feed row was not measured")?,
        conservative_numeric_row_bytes: numeric_upper_bound
            .ok_or("numeric width bound was not measured")?,
        schema_bytes: serde_json::to_vec(&schema)?.len(),
        schema_statements: schema.len(),
    };
    snapshot.close().await?;
    store.close().await;
    Ok(result)
}

#[tokio::test]
async fn maximum_record_rows_and_reconstructed_schema_fit_storage_budgets() -> TestResult {
    // Arrange: all supplied fields reach the current byte/count limits. More
    // record kinds join this permanent case when their bounds are settled.
    let state = project_state()?;
    let mut described = state.clone();
    described.description = Description::try_from("\0".repeat(MAX_DESCRIPTION_BYTES))?;
    described.revision = SubjectRevision::try_from(i64::MAX as u64)?;
    described.closed_at = Some(FeedCursor {
        epoch: EpochNumber::try_from(i64::MAX as u64)?,
        position: FeedPosition::try_from(i64::MAX as u64)?,
    });
    let cases = [
        ("post", maximum_post(&state)?),
        ("project", Record::Project(described)),
        ("workTrailRow16Links", maximum_trail(&state, TRAIL_LINKS)?),
    ];

    // Act: write through the real native owner and use its exact snapshot rows.
    let mut sizes = Vec::new();
    for (kind, record) in cases {
        sizes.push(measure_native_row(kind, &state, record).await?);
    }
    eprintln!(
        "BOARD_RECORD_ENCODED_SIZES {}",
        serde_json::to_string(&sizes)?
    );

    // Assert: schema reconstruction, rather than migration source text, owns
    // its budget; the full encoded SQL row owns the per-record budget.
    for size in sizes {
        assert!(size.schema_statements > 0);
        assert!(
            size.schema_bytes <= SCHEMA_BYTE_BUDGET,
            "{} schema is {} bytes",
            size.kind,
            size.schema_bytes
        );
        assert!(
            size.conservative_numeric_row_bytes <= RECORD_BYTE_BUDGET,
            "{} conservative native feed row is {} bytes, over {}",
            size.kind,
            size.conservative_numeric_row_bytes,
            RECORD_BYTE_BUDGET
        );
    }
    Ok(())
}

#[tokio::test]
async fn history_growing_records_are_measured_without_capping_or_truncating() -> TestResult {
    // These are constructed heavy native-storage cases, not measured production
    // volumes or a claim that the admission operation produced the whole history.
    let state = project_state()?;
    let mut measured = Vec::new();
    for items in [1_000, 10_000, 20_000] {
        let cases = [
            ("dependencyCycle", dependency_cycle(&state, items)?),
            ("subjectAssignments", subject_assignments(&state, items)?),
            ("denseSubtaskKey", dense_subtask_key(&state, items)?),
        ];
        for (kind, record) in cases {
            let key_bytes = match &record {
                Record::Subtask(subtask) => Some(String::from(subtask.place.clone()).len()),
                _ => None,
            };
            let size = measure_native_row(kind, &state, record).await?;
            measured.push(serde_json::json!({
                "kind":kind,"items":items,"encodedKeyBytes":key_bytes,
                "feedRowBytes":size.feed_row_bytes
            }));
        }
        // The header is committed only after genuine request-tagged historical
        // versions exist; a synthetic vector cannot prove storage admission.
        let size = measure_native_row_with_effects(
            "acceptedEffects",
            &state,
            ledger(
                &state,
                StoredDecision::accepted(Vec::new(), ChangeOutcome::Changed),
            )?,
            items,
        )
        .await?;
        measured.push(serde_json::json!({
            "kind":"acceptedEffects", "items":items, "feedRowBytes":size.feed_row_bytes
        }));
    }
    eprintln!(
        "BOARD_DERIVED_RECORD_ENCODED_SIZES {}",
        serde_json::to_string(&measured)?
    );
    Ok(())
}
