//! The real control socket must publish setup before it releases the removal owner.
use super::super::{effect_ports::FixtureGate, owned_state::OwnedQuotaState};
use super::*;
use agent_router_quota_tui::{
    QuotaAccountAction, QuotaAccountCommand, QuotaAccountCommandResult, QuotaAccountRefusal,
};
use std::{fs::File, io::Write, path::Path};
use tokio::net::UnixStream;

struct MutationFlushObserver {
    file: File,
    pending: Vec<u8>,
    gate: FixtureGate,
    release_observations: Arc<Mutex<Vec<bool>>>,
    fail_mutation_flush: bool,
}
impl Write for MutationFlushObserver {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.file.write(bytes)?;
        self.pending.extend_from_slice(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        let record: Value = serde_json::from_slice(&self.pending)?;
        self.pending.clear();
        if record["event"]["operation"] == "reenable-before-removal" {
            self.release_observations
                .lock()
                .map_err(|_| io::Error::other("mutation flush observations poisoned"))?
                .push(self.gate.was_released());
            if self.fail_mutation_flush {
                return Err(io::Error::other("mutation flush failed"));
            }
        }
        self.file.flush()
    }
}

fn observed_journal(
    output: &Path,
    effects: &AccountEffects,
    fail_mutation_flush: bool,
) -> (FixtureEvents, Arc<Mutex<Vec<bool>>>) {
    let release_observations = Arc::new(Mutex::new(Vec::new()));
    let events = FixtureEvents::with_writer(MutationFlushObserver {
        file: File::create_new(output.join("events.jsonl")).unwrap(),
        pending: Vec::new(),
        gate: effects.removal_gate.clone(),
        release_observations: release_observations.clone(),
        fail_mutation_flush,
    });
    (events, release_observations)
}

fn control_server(
    output: &Path,
    state: &OwnedQuotaState,
    accounts: AccountEffects,
    events: FixtureEvents,
) -> FixtureControlServer {
    let socket = output.join("control.sock");
    FixtureControlServer {
        listener: UnixListener::bind(&socket).unwrap(),
        config: FixtureConfiguration {
            scenario: Scenario::RemoveRequiresDisabled,
            output_directory: output.to_owned(),
            events: output.join("events.jsonl"),
            receipt: output.join("receipt.json"),
            control_socket: socket,
            dispatch_receipt: output.join("dispatch.jsonl"),
            login_receipt: None,
            login_gate_socket: None,
            login_standin: None,
        },
        controller: QuotaFixtureController::default(),
        root: state.root().to_owned(),
        target: state.target.clone(),
        accounts,
        refresh: None,
        events,
        mutations: Arc::new(Mutex::new(Vec::new())),
    }
}

#[tokio::test]
async fn reenable_control_publishes_snapshot_before_releasing_real_removal_owner() {
    // Arrange: park the real owner, so the regression fails on ordering, not SQLite contention.
    let output = tempfile::tempdir().unwrap();
    let state = OwnedQuotaState::create(Scenario::RemoveRequiresDisabled)
        .await
        .unwrap();
    let effects = AccountEffects::default();
    let (journal, release_observations) = observed_journal(output.path(), &effects, false);
    let commands = effects.commands(
        state.root().to_owned(),
        Scenario::RemoveRequiresDisabled,
        journal.clone(),
    );
    let mut owner = std::pin::pin!(commands(QuotaAccountCommand {
        target: state.target.clone(),
        action: QuotaAccountAction::Remove,
    }));
    assert!(futures_util::poll!(owner.as_mut()).is_pending());
    assert!(effects.removal_gate.was_entered());
    let server = control_server(output.path(), &state, effects.clone(), journal);
    let (stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(server.serve(stopped));
    let (reader, mut writer) = UnixStream::connect(output.path().join("control.sock"))
        .await
        .unwrap()
        .into_split();
    let mut reader = BufReader::new(reader);

    // Act: use the actual request parser, mutation, snapshot and journal serializer.
    writer
        .write_all(b"{\"operation\":\"reenable-before-removal\",\"requestId\":1}\n")
        .await
        .unwrap();
    let mut line = String::new();
    assert!(reader.read_line(&mut line).await.unwrap() > 0);
    stop.send(()).unwrap();
    serving.await.unwrap().unwrap();
    let response: Value = serde_json::from_str(&line).unwrap();
    let result = owner.await;

    // Assert: only publication releases the owner, which keeps the real refusal.
    assert_eq!(*release_observations.lock().unwrap(), vec![false]);
    assert!(effects.removal_gate.was_released());
    assert_eq!(response["kind"], "fixture-mutation");
    assert_eq!(response["operation"], "reenable-before-removal");
    assert_eq!(response["requestId"], 1);
    assert_eq!(response["accounts"][0]["status"], "enabled");
    assert_eq!(response["accounts"][1]["status"], "enabled");
    assert_eq!(
        result,
        QuotaAccountCommandResult::Refused {
            reason: QuotaAccountRefusal::RequiresDisabled,
        }
    );
    let accounts = read_accounts(state.root()).await.unwrap();
    assert_eq!(accounts.len(), 2);
    assert!(accounts.iter().all(|account| account.status == "enabled"));
    assert_eq!(effects.records.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn reenable_control_flush_failure_keeps_real_removal_owner_held() {
    // Arrange: fail only publication of the mutation after its real database snapshot.
    let output = tempfile::tempdir().unwrap();
    let state = OwnedQuotaState::create(Scenario::RemoveRequiresDisabled)
        .await
        .unwrap();
    let effects = AccountEffects::default();
    let (journal, release_observations) = observed_journal(output.path(), &effects, true);
    let commands = effects.commands(
        state.root().to_owned(),
        Scenario::RemoveRequiresDisabled,
        journal.clone(),
    );
    let mut owner = std::pin::pin!(commands(QuotaAccountCommand {
        target: state.target.clone(),
        action: QuotaAccountAction::Remove,
    }));
    assert!(futures_util::poll!(owner.as_mut()).is_pending());
    assert!(effects.removal_gate.was_entered());
    let server = control_server(output.path(), &state, effects.clone(), journal);
    let (_stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(server.serve(stopped));
    let (reader, mut writer) = UnixStream::connect(output.path().join("control.sock"))
        .await
        .unwrap()
        .into_split();
    let mut reader = BufReader::new(reader);

    // Act: the real server reaches the injected journal flush error.
    writer
        .write_all(b"{\"operation\":\"reenable-before-removal\",\"requestId\":1}\n")
        .await
        .unwrap();
    let mut line = String::new();
    let received = reader.read_line(&mut line).await.unwrap();
    let error = serving.await.unwrap().unwrap_err();

    // Assert: failed publication cannot report success or release the owner.
    assert_eq!(error.to_string(), "mutation flush failed");
    assert_eq!(received, 0);
    assert_eq!(*release_observations.lock().unwrap(), vec![false]);
    assert!(!effects.removal_gate.was_released());
    assert!(effects.records.lock().unwrap().is_empty());
    assert!(futures_util::poll!(owner.as_mut()).is_pending());
}
