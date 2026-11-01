//! Explicit fixture setup has its own acknowledgements; InputOwner owns user receipts.
use crate::{
    AccountActionTarget, QuotaStatusViewModelLoader, quota_control_context::QuotaControlContext,
};
use agent_collaboration_tui::{
    controls::list::ListSelectionControl,
    surfaces::{
        hit_regions::HitMapSnapshot, surface_handle::SurfaceStackHandle,
        surface_stack::SurfaceStack,
    },
};
use codex_router_core::ids::AccountId;
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuotaCapturedContext {
    Details(AccountActionTarget),
    Options {
        account_id: AccountId,
        account_label: String,
    },
    Floor {
        account_id: AccountId,
        account_label: String,
    },
    Other,
}

#[derive(Clone)]
pub struct QuotaFixtureReadback {
    pub mount: u64,
    pub browse_account: Option<AccountId>,
    pub list_selection: Option<String>,
    pub captured: Option<QuotaCapturedContext>,
    pub view_model: QuotaStatusViewModel,
    pub surfaces: SurfaceStack,
    pub actions: Vec<agent_collaboration_tui::actions::ActionPresentation>,
    pub published: Option<HitMapSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuotaFixtureOperation {
    SelectBrowseAccount,
    ReloadViewModel,
}

/// A setup acknowledgement is never a native input or terminal-flush receipt.
#[derive(Clone)]
pub struct QuotaFixtureControlAck {
    pub operation_id: u64,
    pub operation: QuotaFixtureOperation,
    pub readback: QuotaFixtureReadback,
}

pub struct QuotaFixturePendingAck(oneshot::Receiver<Result<QuotaFixtureControlAck, String>>);
impl QuotaFixturePendingAck {
    pub async fn acknowledged(self) -> Result<QuotaFixtureControlAck, String> {
        self.0
            .await
            .map_err(|_| "Fixture mount ended before setup publication".to_owned())?
    }
}

type AckSender = oneshot::Sender<Result<QuotaFixtureControlAck, String>>;
struct PendingOperation {
    id: u64,
    operation: QuotaFixtureOperation,
    sender: AckSender,
}
#[derive(Clone)]
pub(crate) struct LiveQuotaFixture {
    pub(crate) context: QuotaControlContext,
    pub(crate) list: ListSelectionControl,
    pub(crate) stack: SurfaceStackHandle,
    pub(crate) loader: Option<QuotaStatusViewModelLoader>,
    pub(crate) reload_lock: Arc<tokio::sync::Mutex<()>>,
}
#[derive(Default)]
struct ControllerState {
    mount: u64,
    next_operation: u64,
    live: Option<LiveQuotaFixture>,
    pending: Vec<PendingOperation>,
    last_published: Option<QuotaFixtureReadback>,
    publications: Vec<tokio::sync::mpsc::UnboundedSender<QuotaFixtureReadback>>,
}
#[derive(Clone, Default)]
pub struct QuotaFixtureController(Arc<Mutex<ControllerState>>);

impl QuotaFixtureController {
    pub fn subscribe(&self) -> tokio::sync::mpsc::UnboundedReceiver<QuotaFixtureReadback> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .publications
            .push(sender);
        receiver
    }

    pub fn last_published(&self) -> Option<QuotaFixtureReadback> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .last_published
            .clone()
    }

    pub fn readback(&self) -> Result<QuotaFixtureReadback, String> {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let live = state.live.as_ref().ok_or("Quota fixture is not mounted")?;
        Ok(live.readback(state.mount))
    }

    fn live(&self, mount: u64) -> Result<LiveQuotaFixture, String> {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if mount != state.mount {
            return Err("Stale quota fixture mount".to_owned());
        }
        state
            .live
            .clone()
            .ok_or("Quota fixture is not mounted".to_owned())
    }

    fn pending(
        &self,
        mount: u64,
        operation: QuotaFixtureOperation,
    ) -> Result<QuotaFixturePendingAck, String> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.mount != mount || state.live.is_none() {
            return Err("Stale quota fixture mount".to_owned());
        }
        if !state.pending.is_empty() {
            return Err("Fixture setup must await its prior publication".to_owned());
        }
        state.next_operation = state
            .next_operation
            .checked_add(1)
            .ok_or("Fixture operation IDs exhausted")?;
        let id = state.next_operation;
        let (sender, receiver) = oneshot::channel();
        state.pending.push(PendingOperation {
            id,
            operation,
            sender,
        });
        Ok(QuotaFixturePendingAck(receiver))
    }

    pub fn select_browse_account(
        &self,
        mount: u64,
        account: AccountId,
    ) -> Result<QuotaFixturePendingAck, String> {
        let live = self.live(mount)?;
        let visible = live
            .context
            .filters
            .read()
            .committed
            .visible_rows(&live.context.view_model.read().rows);
        if !visible.iter().any(|row| row.account_id == account) {
            return Err("Fixture account is not in the actual visible list".to_owned());
        }
        let pending = self.pending(mount, QuotaFixtureOperation::SelectBrowseAccount)?;
        let mut selected = live.context.focused_account_id;
        selected.set(Some(account));
        Ok(pending)
    }

    pub async fn reload_view_model(&self, mount: u64) -> Result<QuotaFixturePendingAck, String> {
        let live = self.live(mount)?;
        let loader = live
            .loader
            .as_ref()
            .ok_or("Quota fixture has no reload owner")?;
        let _guard = live.reload_lock.lock().await;
        let next = loader()
            .await
            .ok_or("Quota fixture reload owner returned no model")?;
        self.live(mount)?;
        let pending = self.pending(mount, QuotaFixtureOperation::ReloadViewModel)?;
        let mut context = live.context;
        crate::quota_account_options::install_reloaded_view_model(
            next,
            &mut context.view_model,
            &mut context.account_options,
            &context.reset_snapshot,
            &mut context.reset_target,
            context.reset_intents.as_ref(),
        );
        Ok(pending)
    }

    pub(crate) fn mount(&self, live: LiveQuotaFixture) -> Result<u64, String> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.live.is_some() {
            return Err("one controller cannot own two mounted quota views".to_owned());
        }
        state.mount = state
            .mount
            .checked_add(1)
            .ok_or("fixture mount IDs exhausted")?;
        state.live = Some(live);
        Ok(state.mount)
    }

    pub(crate) fn published(&self, mount: u64) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.mount != mount {
            return;
        }
        let Some(live) = state.live.as_ref() else {
            return;
        };
        let readback = live.readback(mount);
        if readback.published.is_none() {
            return;
        }
        state.last_published = Some(readback.clone());
        state
            .publications
            .retain(|sender| sender.send(readback.clone()).is_ok());
        for pending in state.pending.drain(..) {
            let _ = pending.sender.send(Ok(QuotaFixtureControlAck {
                operation_id: pending.id,
                operation: pending.operation,
                readback: readback.clone(),
            }));
        }
    }

    pub(crate) fn unmount(&self, mount: u64) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.mount == mount {
            state.live = None;
            for pending in state.pending.drain(..) {
                let _ = pending.sender.send(Err(
                    "Quota fixture unmounted before setup publication".to_owned(),
                ));
            }
        }
    }
}

impl LiveQuotaFixture {
    fn readback(&self, mount: u64) -> QuotaFixtureReadback {
        use crate::quota_control_context::QuotaDialogKind;
        let captured = self
            .context
            .dialog
            .read()
            .as_ref()
            .map(|session| match &session.kind {
                QuotaDialogKind::Account(target) => QuotaCapturedContext::Details(target.clone()),
                QuotaDialogKind::Options => self.context.account_options.read().as_ref().map_or(
                    QuotaCapturedContext::Other,
                    |options| QuotaCapturedContext::Options {
                        account_id: options.target.account_id.clone(),
                        account_label: options.target.account_label.clone(),
                    },
                ),
                QuotaDialogKind::Floor => self.context.weekly_floor_editor.read().as_ref().map_or(
                    QuotaCapturedContext::Other,
                    |editor| QuotaCapturedContext::Floor {
                        account_id: editor.account_id.clone(),
                        account_label: editor.account_label.clone(),
                    },
                ),
                _ => QuotaCapturedContext::Other,
            });
        let surfaces = self.stack.snapshot();
        let actions = surfaces.top().actions.presentations();
        QuotaFixtureReadback {
            mount,
            browse_account: self.context.focused_account_id.read().clone(),
            list_selection: self.list.selected().map(|key| key.as_str().to_owned()),
            captured,
            view_model: self.context.view_model.read().clone(),
            surfaces,
            actions,
            published: self.stack.hit_regions().published(),
        }
    }
}
