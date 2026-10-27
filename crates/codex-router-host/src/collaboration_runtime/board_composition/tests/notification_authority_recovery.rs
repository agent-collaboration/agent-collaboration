//! A real renewal restores quiet publication; typed NotWriter keeps SDK custody.
use super::*;
use collaboration_contracts::ProjectReplicationPosition;
use fleet_sync_delivery::OutboxStatusState;
use fleet_sync_protocol::{
    CommitSequence, DeliveryStatus, Epoch, LeaseRefusal, LeaseState, LeaseStatus, RenewAnswer,
    RenewResult, ReplicationPosition,
};
use runtime_primitives::{Clock, SyncTime, SystemClock};
use std::{collections::BTreeMap, io::Write};
use tracing::field::{Field, Visit};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

#[derive(Clone, Debug)]
struct Diagnostic {
    level: tracing::Level,
    fields: BTreeMap<String, String>,
}
#[derive(Default)]
struct Fields(BTreeMap<String, String>);
impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().into(), value.to_string());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().into(), value.to_string());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}
struct Capture {
    events: Arc<std::sync::Mutex<Vec<Diagnostic>>>,
    changed: watch::Sender<()>,
}
impl<S: tracing::Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if !matches!(
            *event.metadata().level(),
            tracing::Level::WARN | tracing::Level::ERROR
        ) {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        self.events.lock().unwrap().push(Diagnostic {
            level: *event.metadata().level(),
            fields: fields.0,
        });
        self.changed.send_replace(());
    }
}
async fn wait_diagnostic(
    events: &Arc<std::sync::Mutex<Vec<Diagnostic>>>,
    changed: &watch::Sender<()>,
    project: &ProjectId,
    message: &str,
    level: tracing::Level,
) -> Result<Diagnostic, TestError> {
    let mut notification = changed.subscribe();
    tokio::time::timeout(BOUND, async {
        loop {
            if let Some(event) = events
                .lock()
                .unwrap()
                .iter()
                .find(|event| {
                    event.level == level
                        && event
                            .fields
                            .get("project")
                            .is_some_and(|value| value == project.as_str())
                        && event
                            .fields
                            .get("message")
                            .is_some_and(|value| value.contains(message))
                })
                .cloned()
            {
                return Ok::<_, TestError>(event);
            }
            notification.changed().await?;
        }
    })
    .await
    .map_err(|_| io::Error::other(format!("missing {level} diagnostic {message}")))?
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn real_renewal_republishes_after_paused_pass_and_not_writer_reports_confirmed_and_lost_authority()
-> Result<(), TestError> {
    // A separate exact-case process gives log capture its own global subscriber,
    // independent of unrelated tests' logger initialization and runtime tasks.
    const CHILD: &str = "CODEX_ROUTER_AUTHORITY_RECOVERY_PROOF_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let registered = concat!(
            module_path!(),
            "::real_renewal_republishes_after_paused_pass_and_not_writer_reports_confirmed_and_lost_authority"
        );
        let (_, registered) = registered.split_once("::").ok_or("missing crate prefix")?;
        let output = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", registered, "--nocapture"])
            .env(CHILD, "1")
            .output()?;
        std::io::stdout().write_all(&output.stdout)?;
        std::io::stderr().write_all(&output.stderr)?;
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
            "child selected no exact-case test"
        );
        assert!(
            output.status.success(),
            "isolated native authority proof failed: {}",
            output.status
        );
        return Ok(());
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(assert_recovery())
}

async fn assert_recovery() -> Result<(), TestError> {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let changed = watch::channel(()).0;
    tracing_subscriber::registry()
        .with(Capture {
            events: events.clone(),
            changed: changed.clone(),
        })
        .try_init()?;
    let directory = tempfile::tempdir()?;
    let mut fixture = Box::pin(ProjectNotificationFixture::open(directory.path())).await?;
    let writer = fixture.harness.projects.ensure(&fixture.project).await?;
    let item_reference: message_board::MailItemRef =
        fixture.seed_item.as_uuid().to_string().try_into()?;
    let mut frozen_read = writer.store.snapshot().await?;
    let frozen = frozen_read
        .mail_handoff(&item_reference)
        .await?
        .ok_or("missing frozen handoff")?;
    let message_board_storage::MailHandoffItem::Delivery(original) = frozen.item else {
        return Err("expected frozen delivery".into());
    };
    // Compare canonical serde envelope bytes, not transport frame formatting.
    let original_bytes = serde_json::to_vec(original.as_ref())?;
    let original_reference = fleet_sync_protocol::DeliveryItemRef {
        item: original.id(),
        digest: original.digest().clone(),
    };
    assert_eq!(original_reference.item, fixture.seed_item);
    frozen_read.finish().await?;
    assert!(
        !fixture
            .harness
            .mail
            .rows()
            .await?
            .iter()
            .any(|row| row.category == "outbox"),
        "unexpected initial outgoing custody"
    );
    assert!(
        fixture.harness.mail.pending_submissions().await?.is_empty(),
        "unexpected initial pending Submit operation"
    );
    // Real replication makes the frozen item eligible. Discard only its returned
    // prompt while the actual Welcome is still held before the Host event loop.
    let acknowledgement = writer
        .sender
        .replicate_once(fixture.harness.central.receiver)
        .await?;
    assert!(acknowledgement.iter().any(|event| matches!(
        event,
        fleet_sync_writer::ReplicationEvents::Acknowledged { .. }
    )));
    assert_eq!(
        fixture
            .harness
            .projects
            .acknowledged_mail_changes(&fixture.project)
            .await?
            .max_handoff_seq,
        Some(1)
    );
    let database = fixture.harness.registry.database(&fixture.project).await?;
    let started = SyncTime::from_unix_millis(SystemClock.now().unix_millis() - 120_000);
    let old = LeaseStatus {
        database: database.database(),
        epoch: Epoch::new(fixture.seed_epoch.get())?,
        holder: fixture.harness.peer(),
        state: LeaseState::Held,
        expires_at: started.after(Duration::from_secs(60)),
        remaining_ms: 60_000,
    };
    database
        .observe_renewal(
            RenewAnswer::Renewed {
                recorded: RenewResult {
                    epoch: old.epoch,
                    expires_at: old.expires_at,
                },
                current: old,
                transfer_in_progress: None,
                replayed: false,
            },
            started,
        )
        .await?;
    assert!(matches!(
        writer.store.writer_state().await?,
        fleet_sync_lease::WriterState::Paused { .. }
    ));
    let (renewal_entered, release_renewal) = fixture.harness.transport.observer.hold_next_renewal();
    fixture
        .start()
        .await
        .map_err(|error| io::Error::other(format!("starting real Welcome: {error}")))?;

    wait_diagnostic(
        &events,
        &changed,
        &fixture.project,
        "frozen handoff publication delayed",
        tracing::Level::WARN,
    )
    .await?;
    assert!(
        matches!(
            fixture
                .harness
                .mail
                .outbox_status(fixture.seed_item)
                .await?
                .state,
            OutboxStatusState::Unknown
        ),
        "Paused writer admitted outgoing custody"
    );
    assert!(
        !fixture
            .harness
            .central
            .notifications
            .has_submission(fixture.seed_item)
            .await
    );

    // Artificially expiring a cached observation does not advance the already
    // scheduled renewal journal. Prompt the ordinary public SDK renewal now;
    // actual transport, registry answer and authority observation remain real.
    writer.runtime.prompt_renewal().await?;
    tokio::time::timeout(BOUND, renewal_entered)
        .await
        .map_err(|_| {
            io::Error::other(format!(
                "waiting for real renewal reply; wire={:?}; diagnostics={:?}",
                fixture.harness.transport.observer.events(),
                events.lock().unwrap()
            ))
        })??;
    assert!(fixture.harness.transport.observer.events().iter().any(|event|matches!(event,transport::WireEvent::Renewal(epoch) if epoch.value()==fixture.seed_epoch.get())));

    // No board mutation, copy acknowledgement, explicit publisher prompt or
    // unrelated hint follows. This is the actual generic registry renewal reply.
    release_renewal
        .send(())
        .map_err(|_| io::Error::other("renewal gate closed"))?;
    tokio::time::timeout(
        BOUND,
        fixture
            .harness
            .central
            .notifications
            .wait_submission(fixture.seed_item),
    )
    .await
    .map_err(|_| {
        io::Error::other(format!(
            "publication after actual renewal; wire={:?}; diagnostics={:?}",
            fixture.harness.transport.observer.events(),
            events.lock().unwrap()
        ))
    })?;
    let confirmed = wait_diagnostic(
        &events,
        &changed,
        &fixture.project,
        "while this holder is confirmed",
        tracing::Level::ERROR,
    )
    .await?;
    assert_eq!(
        confirmed.fields.get("submitted_epoch"),
        Some(&fixture.seed_epoch.get().to_string())
    );
    assert_eq!(
        confirmed.fields.get("confirmed_epoch"),
        Some(&fixture.seed_epoch.get().to_string())
    );
    assert!(
        confirmed
            .fields
            .get("own_peer")
            .is_some_and(|value| value.contains(&fixture.harness.peer().as_uuid().to_string()))
    );
    assert!(matches!(
        writer.store.writer_state().await?,
        fleet_sync_lease::WriterState::Writing { .. }
    ));
    // Pinned SDK AnswerLost retry precondition, not a publication cardinality.
    fixture
        .harness
        .transport
        .observer
        .wait_for(
            |events| {
                events
                    .iter()
                    .filter(|event| {
                        matches!(event,
                            transport::WireEvent::MailSubmit(item) if *item == fixture.seed_item
                        )
                    })
                    .count()
                    >= 2
            },
            "same frozen item did not produce a second actual submission attempt",
        )
        .await?;
    let confirmed_submission =
        assert_frozen_submission(&fixture, &original_reference, &original_bytes).await?;
    let shown = fixture
        .application
        .messages()
        .message_show(collaboration_protocol::MessageShowRequest {
            caller: collaboration_protocol::MessageReadCaller::Human(
                collaboration_protocol::HumanMessageReader::HumanUser,
            ),
            reference: fixture.seed_item.as_uuid().to_string(),
        })
        .await?;
    assert!(
        matches!(shown,collaboration_protocol::MessageShowResult::Remote { mail } if matches!(&mail.context,collaboration_protocol::MailReadContext::Project { project,.. } if project==&fixture.project) && !matches!(mail.status,collaboration_protocol::RemoteDeliveryStatus::Refused { .. }))
    );

    let mut frozen_read = writer.store.snapshot().await?;
    let readback = frozen_read
        .mail_handoff(&item_reference)
        .await?
        .ok_or("frozen handoff disappeared")?;
    let message_board_storage::MailHandoffItem::Delivery(readback) = readback.item else {
        return Err("frozen delivery changed kind".into());
    };
    assert_eq!(serde_json::to_vec(readback.as_ref())?, original_bytes);
    frozen_read.finish().await?;

    // Lose actual persisted authority while the same SDK item remains queued.
    // The next typed rejection must warn, without upgrading unknown authority.
    database
        .observe_renewal(
            RenewAnswer::Refused(LeaseRefusal::StaleEpoch {
                current_epoch: Epoch::new(fixture.seed_epoch.get() + 1)?,
                epoch_ended_at: ReplicationPosition::new(
                    Epoch::new(fixture.seed_epoch.get())?,
                    CommitSequence::new(0),
                ),
            }),
            SystemClock.now(),
        )
        .await?;
    let lost = wait_diagnostic(
        &events,
        &changed,
        &fixture.project,
        "authority; custody retained",
        tracing::Level::WARN,
    )
    .await?;
    assert_eq!(
        lost.fields.get("submitted_epoch"),
        Some(&fixture.seed_epoch.get().to_string())
    );
    assert!(
        !lost.fields.contains_key("confirmed_epoch"),
        "lost authority was represented as confirmed"
    );
    let after_loss = fixture
        .harness
        .transport
        .observer
        .events()
        .iter()
        .filter(|event| matches!(event, transport::WireEvent::SubmissionEpoch { .. }))
        .count();
    fixture
        .harness
        .transport
        .observer
        .wait_for(
            |events| {
                events.iter().filter(|event| matches!(event,
            transport::WireEvent::SubmissionEpoch { item, .. } if *item == fixture.seed_item
        )).count() > after_loss
            },
            "no further real dispatch after the lost-authority diagnostic",
        )
        .await?;
    assert!(matches!(
        fixture
            .harness
            .mail
            .outbox_status(fixture.seed_item)
            .await?
            .state,
        OutboxStatusState::Known {
            status: DeliveryStatus::Queued,
            ..
        }
    ));
    let lost_submission =
        assert_frozen_submission(&fixture, &original_reference, &original_bytes).await?;
    assert_eq!(
        confirmed_submission, lost_submission,
        "authority loss changed logical submission identity"
    );
    eprintln!(
        "native renewal recovered one frozen custody/admission across repeated wire attempts; confirmed and lost NotWriter diagnostics: {confirmed:?}; {lost:?}"
    );
    drop(writer);
    drop(database);
    fixture.close().await
}

#[derive(Debug, Eq, PartialEq)]
struct StableSubmission {
    id: runtime_primitives::OperationId,
    kind: String,
    version: u32,
    key: durable_steps::RequestKey,
    input: Vec<u8>,
    digest: Vec<u8>,
    steps: Vec<(u64, runtime_primitives::StepName, StableStep)>,
}

#[derive(Debug, Eq, PartialEq)]
enum StableStep {
    Local(Vec<u8>),
    Pending(durable_steps::storage::CallIntent),
}

async fn assert_frozen_submission(
    fixture: &ProjectNotificationFixture,
    reference: &fleet_sync_protocol::DeliveryItemRef,
    original_bytes: &[u8],
) -> Result<StableSubmission, TestError> {
    let mail = &fixture.harness.mail;
    let rows = mail.rows().await?;
    let custody = rows
        .iter()
        .filter(|row| row.category == "outbox")
        .collect::<Vec<_>>();
    assert_eq!(
        custody.len(),
        1,
        "duplicate or missing SDK outgoing custody"
    );
    assert_eq!(
        custody[0].metadata.as_deref(),
        Some("1"),
        "custody is not live"
    );
    assert_eq!(custody[0].identity, reference.item.as_uuid().to_string());
    assert!(matches!(
        mail.outbox_status(reference.item).await?.state,
        OutboxStatusState::Known {
            status: DeliveryStatus::Queued,
            ..
        }
    ));
    let epoch = mail
        .owner
        .mail_submission_epoch(&reference.item.as_uuid().to_string().try_into()?)
        .await?
        .ok_or("missing custody epoch")?;
    assert_eq!(epoch.project, fixture.project);
    assert_eq!(epoch.epoch, fixture.seed_epoch);
    let envelopes = fixture
        .harness
        .central
        .notifications
        .submitted_envelopes(reference.item)
        .await;
    assert!(!envelopes.is_empty(), "no actual consumer receipt");
    for envelope in envelopes {
        assert_eq!(envelope, original_bytes, "retry changed frozen bytes");
    }
    let mut epochs = Vec::new();
    for event in fixture.harness.transport.observer.events() {
        match event {
            transport::WireEvent::MailSubmit(item) => {
                assert_eq!(item, reference.item, "unexpected submitted identity")
            }
            transport::WireEvent::SubmissionEpoch { item, epoch } => {
                assert_eq!(item, reference.item, "unexpected epoch identity");
                epochs.push(epoch);
            }
            transport::WireEvent::GenericSubmit(_) => {
                return Err("project submission bypassed authority adapter".into());
            }
            _ => {}
        }
    }
    assert!(epochs.len() >= 2, "actual repeated attempts missing");
    assert!(
        epochs
            .iter()
            .all(|epoch| *epoch == Some(fixture.seed_epoch.get())),
        "retry changed submitted authority epoch: {epochs:?}"
    );
    let mut operations = mail.pending_submissions().await?;
    let resolved = mail.resolve_submit(reference.clone()).await?;
    assert_eq!(
        serde_json::to_vec(&resolved.item)?,
        original_bytes,
        "custody changed frozen payload"
    );
    assert_eq!(
        operations.len(),
        1,
        "duplicate or missing pending SDK Submit operation"
    );
    let operation = operations.pop().ok_or("missing submit operation")?;
    assert_eq!(
        operation.request_key,
        fleet_sync_delivery::delivery_request_key(&reference.item)?
    );
    let durable_steps::storage::OperationExecution::Running { input, .. } = operation.execution
    else {
        return Err("submit operation is not pending".into());
    };
    let submitted: fleet_sync_delivery::SubmitInput = serde_json::from_slice(&input)?;
    assert_eq!(submitted.item, reference.item);
    assert_eq!(submitted.digest, reference.digest);
    assert_eq!(submitted.relay, resolved.relay);
    assert_eq!(submitted.mailbox, resolved.item.recipient().peer);
    let (record, steps) = mail.operation(operation.id).await?;
    assert_eq!(record.ok_or("submit disappeared")?.id, operation.id);
    let mut stable_steps = Vec::new();
    let mut pending_calls = 0;
    for step in steps {
        assert_eq!(step.operation, operation.id);
        let state = match step.state {
            durable_steps::storage::StepState::Local { output } => StableStep::Local(output),
            durable_steps::storage::StepState::CallPending { intent, attempt } => {
                assert!(matches!(
                    attempt.reach,
                    durable_steps::storage::CallReach::Dispatching
                        | durable_steps::storage::CallReach::MayHaveReached
                ));
                pending_calls += 1;
                StableStep::Pending(intent)
            }
            durable_steps::storage::StepState::CallDone { .. } => {
                return Err("NotWriter falsely settled a call".into());
            }
        };
        stable_steps.push((step.position, step.name, state));
    }
    assert_eq!(pending_calls, 1, "expected one pending remote submit call");
    Ok(StableSubmission {
        id: operation.id,
        kind: operation.kind,
        version: operation.version,
        key: operation.request_key,
        input,
        digest: operation.input_digest,
        steps: stable_steps,
    })
}
