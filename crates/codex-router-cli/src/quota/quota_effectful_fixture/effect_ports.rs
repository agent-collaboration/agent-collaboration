//! Keep accepted in-process effects at their real CLI owners and record their typed results.
use super::{
    configuration::Scenario,
    owned_state::{change_status, read_accounts},
    receipts::{self, FixtureEvents},
};
use agent_router_quota_tui::{
    QuotaAccountAction, QuotaAccountCommand, QuotaAccountCommandResult, QuotaAccountCommands,
};
use serde_json::{Value, json};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Notify;

#[derive(Clone, Default)]
pub(super) struct FixtureGate {
    entered: Arc<AtomicBool>,
    released: Arc<AtomicBool>,
    notify: Arc<Notify>,
    entered_notify: Arc<Notify>,
}
impl FixtureGate {
    pub(super) async fn hold(&self, events: &FixtureEvents, kind: &str) -> io::Result<()> {
        if self.entered.swap(true, Ordering::SeqCst) {
            return Err(io::Error::other("fixture gate was entered twice"));
        }
        events.write(json!({"kind":kind}))?;
        self.entered_notify.notify_waiters();
        while !self.released.load(Ordering::SeqCst) {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.released.load(Ordering::SeqCst) {
                break;
            }
            notified.await;
        }
        Ok(())
    }
    pub(super) async fn wait_until_entered(&self) {
        while !self.entered.load(Ordering::SeqCst) {
            let notified = self.entered_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.entered.load(Ordering::SeqCst) {
                break;
            }
            notified.await;
        }
    }
    pub(super) fn release(&self) -> io::Result<()> {
        if !self.entered.load(Ordering::SeqCst) {
            return Err(io::Error::other("fixture release preceded causal entry"));
        }
        if self.released.swap(true, Ordering::SeqCst) {
            return Err(io::Error::other("fixture gate was released twice"));
        }
        self.notify.notify_waiters();
        Ok(())
    }
    pub(super) fn was_released(&self) -> bool {
        self.released.load(Ordering::SeqCst)
    }
    pub(super) fn was_entered(&self) -> bool {
        self.entered.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Default)]
pub(super) struct AccountEffects {
    pub records: Arc<Mutex<Vec<(QuotaAccountCommand, QuotaAccountCommandResult)>>>,
    pub clipboard: Arc<Mutex<Vec<String>>>,
    pub removal_gate: FixtureGate,
}
impl AccountEffects {
    pub(super) fn commands(
        &self,
        root: PathBuf,
        scenario: Scenario,
        events: FixtureEvents,
    ) -> QuotaAccountCommands {
        let effects = self.clone();
        Arc::new(move |request| {
            let effects = effects.clone();
            let root = root.clone();
            let events = events.clone();
            Box::pin(async move {
                events.write(json!({"kind":"account-owner-started","command":receipts::command(&request)})).unwrap();
                if scenario == Scenario::RemoveRequiresDisabled
                    && request.action == QuotaAccountAction::Remove
                {
                    effects
                        .removal_gate
                        .hold(&events, "removal-before-owner-held")
                        .await
                        .unwrap();
                }
                let copied = effects.clipboard.clone();
                let copied_events = events.clone();
                let clipboard: crate::quota::quota_account_adapter::QuotaClipboardWriter =
                    Arc::new(move |payload| {
                        copied.lock().unwrap().push(payload.to_owned());
                        copied_events.write(json!({"kind":"clipboard-written","payload":payload}))
                    });
                let before_commit = if scenario == Scenario::RemoveHeld {
                    let gate = effects.removal_gate.clone();
                    let events = events.clone();
                    Some(Arc::new(move || {
                        let gate = gate.clone();
                        let events = events.clone();
                        Box::pin(async move {
                            gate.hold(&events, "removal-after-credential-delete-held")
                                .await
                                .unwrap();
                        })
                            as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
                    })
                        as crate::account::account_removal::RemovalCommitHook)
                } else {
                    None
                };
                let result = crate::quota::quota_account_adapter::execute_account_command(
                    root.clone(),
                    request.clone(),
                    clipboard,
                    before_commit,
                )
                .await;
                let persisted = read_accounts(&root).await.unwrap();
                effects
                    .records
                    .lock()
                    .unwrap()
                    .push((request.clone(), result.clone()));
                events.write(json!({"kind":"account-owner-finished","command":receipts::command(&request),"result":receipts::command_result(&result),"accounts":persisted})).unwrap();
                result
            })
        })
    }
    pub(super) async fn reenable_before_owner(
        &self,
        root: &std::path::Path,
        scenario: Scenario,
        target: &codex_router_core::ids::AccountId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if scenario != Scenario::RemoveRequiresDisabled
            || !self.removal_gate.was_entered()
            || self.removal_gate.was_released()
        {
            return Err(io::Error::other(
                "re-enable setup requires the held RequiresDisabled scenario",
            )
            .into());
        }
        change_status(
            root,
            target,
            codex_router_state::account::AccountStatus::Enabled,
        )
        .await?;
        self.removal_gate.release()?;
        Ok(())
    }
    pub(super) fn receipts(&self) -> Vec<Value> {
        self.records.lock().unwrap().iter().map(|(request,result)|json!({"command":receipts::command(request),"result":receipts::command_result(result)})).collect()
    }
}
