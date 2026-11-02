//! One private setup connection; terminal input continues to belong only to InputOwner.
use super::{
    FixtureResult,
    configuration::{FixtureConfiguration, FixtureControlRequest, Scenario},
    effect_ports::AccountEffects,
    owned_state::{change_status, read_accounts, replace_login_generation},
    receipts::{self, FixtureControlReceipt, FixtureEvents},
    refresh_fixture::RefreshEffects,
};
use agent_router_quota_tui::{
    AccountActionTarget,
    test_support::{QuotaCapturedContext, QuotaFixtureController},
};
use codex_router_core::ids::AccountId;
use codex_router_state::account::{AccountRecord, AccountStatus};
use serde_json::{Value, json};
use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixListener,
    sync::oneshot,
};

pub(super) struct FixtureControlServer {
    pub listener: UnixListener,
    pub config: FixtureConfiguration,
    pub controller: QuotaFixtureController,
    pub root: PathBuf,
    pub target: AccountActionTarget,
    pub accounts: AccountEffects,
    pub refresh: Option<RefreshEffects>,
    pub events: FixtureEvents,
    pub mutations: Arc<Mutex<Vec<Value>>>,
}
impl FixtureControlServer {
    pub(super) async fn serve(self, mut stopped: oneshot::Receiver<()>) -> FixtureResult<()> {
        let stream = tokio::select! {
            _=&mut stopped=>return Ok(()),
            connected=self.listener.accept()=>connected?.0,
        };
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut last_request = 0;
        loop {
            let mut line = String::new();
            let read = tokio::select! {
                _=&mut stopped=>return Ok(()),
                read=reader.read_line(&mut line)=>read?,
            };
            if read == 0 {
                return Ok(());
            }
            if line.len() > 4096 {
                return Err(io::Error::other("fixture setup request exceeded limit").into());
            }
            let request: FixtureControlRequest = match serde_json::from_str(&line) {
                Ok(request) => request,
                Err(error) => {
                    let wire: Value = serde_json::from_str(&line)?;
                    if wire["operation"] != "await_event" {
                        return Err(error.into());
                    }
                    let request_id = wire["requestId"]
                        .as_u64()
                        .filter(|&id| id > last_request)
                        .ok_or_else(|| {
                            io::Error::other("fixture request IDs must increase without replay")
                        })?;
                    last_request = request_id;
                    write_response(&mut writer, &json!({"kind":"fixture-control-error","requestId":request_id,"message":error.to_string()})).await?;
                    continue;
                }
            };
            let request_id = match &request {
                FixtureControlRequest::AwaitEvent { request_id, .. }
                | FixtureControlRequest::Readback { request_id }
                | FixtureControlRequest::SelectBrowseAccount { request_id, .. }
                | FixtureControlRequest::ReloadViewModel { request_id, .. }
                | FixtureControlRequest::RemoveCapturedAccountExternally { request_id, .. }
                | FixtureControlRequest::ReenableBeforeRemoval { request_id }
                | FixtureControlRequest::ReleaseRemoval { request_id }
                | FixtureControlRequest::ReleaseRefresh { request_id } => *request_id,
            };
            if request_id <= last_request {
                return Err(
                    io::Error::other("fixture request IDs must increase without replay").into(),
                );
            }
            last_request = request_id;
            if let FixtureControlRequest::AwaitEvent {
                request_id,
                after_ordinal,
                kind,
            } = request
            {
                let journal = self.events.clone();
                let completion = tokio::select! {
                    completion = journal.await_event(after_ordinal, kind) => completion,
                    _ = &mut stopped => {
                        drop(self);
                        let completion = journal.await_event(after_ordinal, kind).await;
                        write_response(&mut writer, &fence_response(request_id, completion)).await?;
                        return Ok(());
                    }
                };
                write_response(&mut writer, &fence_response(request_id, completion)).await?;
                continue;
            }
            let result = self.execute(request).await;
            let response = match result {
                Ok(value) => value,
                Err(error) => {
                    json!({"kind":"fixture-control-error","requestId":request_id,"message":error.to_string()})
                }
            };
            self.events.write(response.clone())?;
            write_response(&mut writer, &response).await?;
        }
    }
    async fn execute(&self, request: FixtureControlRequest) -> FixtureResult<Value> {
        match request {
            FixtureControlRequest::AwaitEvent {
                request_id,
                after_ordinal,
                kind,
            } => Ok(fence_response(
                request_id,
                self.events.await_event(after_ordinal, kind).await,
            )),
            FixtureControlRequest::Readback { request_id } => Ok(
                json!({"kind":"fixture-readback","requestId":request_id,"readback":receipts::readback(&self.controller.readback().map_err(io::Error::other)?),"accounts":read_accounts(&self.root).await?,"accountCommands":self.accounts.receipts(),"refresh":self.refresh.as_ref().map(RefreshEffects::readback)}),
            ),
            FixtureControlRequest::SelectBrowseAccount {
                request_id,
                mount,
                account_id,
            } => {
                let acknowledgement = self
                    .controller
                    .select_browse_account(mount, AccountId::new(account_id)?)
                    .map_err(io::Error::other)?
                    .acknowledged()
                    .await
                    .map_err(io::Error::other)?;
                Ok(serde_json::to_value(FixtureControlReceipt::from_ack(
                    request_id,
                    &acknowledgement,
                ))?)
            }
            FixtureControlRequest::ReloadViewModel { request_id, mount } => {
                let acknowledgement = self
                    .controller
                    .reload_view_model(mount)
                    .await
                    .map_err(io::Error::other)?
                    .acknowledged()
                    .await
                    .map_err(io::Error::other)?;
                Ok(serde_json::to_value(FixtureControlReceipt::from_ack(
                    request_id,
                    &acknowledgement,
                ))?)
            }
            FixtureControlRequest::RemoveCapturedAccountExternally { request_id, mount } => {
                if self.config.scenario != Scenario::CapturedActions
                    || !self.mutations.lock().unwrap().is_empty()
                {
                    return Err(io::Error::other(
                        "external disappearance is a one-shot captured-actions setup",
                    )
                    .into());
                }
                let readback = self.controller.readback().map_err(io::Error::other)?;
                if readback.mount != mount
                    || !matches!(readback.captured,Some(QuotaCapturedContext::Details(ref target)) if target.account_id==self.target.account_id)
                {
                    return Err(io::Error::other(
                        "external disappearance requires actual captured Details A",
                    )
                    .into());
                }
                change_status(&self.root, &self.target.account_id, AccountStatus::Disabled).await?;
                let expected = AccountRecord::new(
                    self.target.provider,
                    self.target.account_id.clone(),
                    self.target.label.as_str(),
                    AccountStatus::Disabled,
                )
                .with_active_credential_generation(7);
                crate::account::account_removal::remove_resolved_account(
                    self.root.clone(),
                    expected,
                )
                .await?;
                let acknowledgement = self
                    .controller
                    .reload_view_model(mount)
                    .await
                    .map_err(io::Error::other)?
                    .acknowledged()
                    .await
                    .map_err(io::Error::other)?;
                let value = json!({"kind":"fixture-mutation","operation":"remove-captured-account-externally","requestId":request_id,"accountId":self.target.account_id.as_str(),"accounts":read_accounts(&self.root).await?,"readback":receipts::readback(&acknowledgement.readback)});
                self.mutations.lock().unwrap().push(value.clone());
                Ok(value)
            }
            FixtureControlRequest::ReenableBeforeRemoval { request_id } => {
                self.accounts
                    .reenable_before_owner(
                        &self.root,
                        self.config.scenario,
                        &self.target.account_id,
                    )
                    .await?;
                let value = json!({"kind":"fixture-mutation","operation":"reenable-before-removal","requestId":request_id,"accounts":read_accounts(&self.root).await?});
                self.mutations.lock().unwrap().push(value.clone());
                Ok(value)
            }
            FixtureControlRequest::ReleaseRemoval { request_id } => {
                if self.config.scenario != Scenario::RemoveHeld {
                    return Err(io::Error::other(
                        "release-removal requires the held removal scenario",
                    )
                    .into());
                }
                self.accounts.removal_gate.release()?;
                Ok(
                    json!({"kind":"fixture-gate-released","operation":"release-removal","requestId":request_id}),
                )
            }
            FixtureControlRequest::ReleaseRefresh { request_id } => {
                let dispatch = std::fs::read_to_string(&self.config.dispatch_receipt)?;
                let last_dispatch: Value =
                    serde_json::from_str(dispatch.lines().last().ok_or_else(|| {
                        io::Error::other("refresh release has no actual input receipt")
                    })?)?;
                let sequence = last_dispatch["sequence"]
                    .as_u64()
                    .ok_or_else(|| io::Error::other("actual input receipt lacks a sequence"))?;
                self.refresh
                    .as_ref()
                    .ok_or_else(|| io::Error::other("scenario has no refresh gate"))?
                    .release()?;
                Ok(
                    json!({"kind":"fixture-gate-released","operation":"release-refresh","requestId":request_id,"actualInputSequenceThrough":sequence}),
                )
            }
        }
    }
}

fn fence_response(request_id: u64, result: super::event_fence::FixtureEventFenceResult) -> Value {
    json!({"kind":"fixture-event-fence","requestId":request_id,"result":result})
}

async fn write_response(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    response: &Value,
) -> FixtureResult<()> {
    writer
        .write_all(serde_json::to_string(response)?.as_bytes())
        .await?;
    writer.write_all(b"\n").await?;
    Ok(())
}

pub(super) async fn run_login_gate(
    listener: UnixListener,
    root: PathBuf,
    target: AccountActionTarget,
    scenario: Scenario,
    events: FixtureEvents,
    mutations: Arc<Mutex<Vec<Value>>>,
) -> FixtureResult<Value> {
    let (stream, _) = listener.accept().await?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let receipt: Value = serde_json::from_str(&line)?;
    if receipt["exitCode"].as_u64() != scenario.login_exit().map(u64::from) {
        return Err(io::Error::other("stand-in's declared exit differs from the scenario").into());
    }
    let expected_arguments = json!([
        "account",
        "login",
        "--provider",
        target.provider.as_str(),
        "--label",
        target.label.as_str(),
        "--router-root",
        root.to_str()
            .ok_or_else(|| io::Error::other("fixture root is not UTF-8"))?
    ]);
    let terminal = &receipt["terminal"];
    if receipt["arguments"] != expected_arguments
        || terminal["kind"] != "interactive"
        || !["canonical", "echo", "signals", "foreground"]
            .iter()
            .all(|flag| terminal[flag] == true)
    {
        return Err(io::Error::other("actual login child argv/terminal ownership differs from the captured target and restored PTY").into());
    }
    events.write(json!({"kind":"login-child-entered","receipt":receipt}))?;

    if scenario == Scenario::LoginSucceeded {
        replace_login_generation(&root, &target.account_id).await?;
        let mutation = json!({"kind":"fixture-mutation","operation":"activate-fresh-login-generation","accounts":read_accounts(&root).await?});
        events.write(mutation.clone())?;
        mutations
            .lock()
            .map_err(|_| io::Error::other("fixture mutation ledger poisoned"))?
            .push(mutation);
    }
    writer.write_all(b"settle\n").await?;
    writer.shutdown().await?;
    Ok(receipt)
}
