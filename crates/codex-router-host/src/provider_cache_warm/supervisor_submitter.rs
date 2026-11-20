//! The real warm-prompt path: the supervisor's prompt admission, refusing a busy session.
use super::{WARM_PROMPT_TEXT, WarmPromptFuture, WarmPromptRefused, WarmPromptSubmitter, WarmRun};
use crate::{
    ExternalProviderSupervisor, ProviderPromptContentsRequest,
    external_provider_supervisor::PromptBusyPolicy, session_owner_calls::InputAcceptance,
};
use collaboration_protocol::{OperationId, SessionRef};
use collaboration_runtime::ProviderOperationStore;
use session_event_model::InputId;
use std::sync::{Arc, Weak};
use tokio::sync::Mutex;

/// Holds the supervisor and the operation store weakly: the supervisor owns the
/// runtime whose event sink holds this module, so strong references would be a
/// cycle and would keep the store owned after Host shutdown.
pub(crate) struct SupervisorWarmPromptSubmitter {
    supervisor: Weak<ExternalProviderSupervisor>,
    store: Weak<Mutex<ProviderOperationStore>>,
}

impl SupervisorWarmPromptSubmitter {
    pub(crate) fn new(
        supervisor: Arc<ExternalProviderSupervisor>,
        store: Arc<Mutex<ProviderOperationStore>>,
    ) -> Self {
        Self {
            supervisor: Arc::downgrade(&supervisor),
            store: Arc::downgrade(&store),
        }
    }
}

impl WarmPromptSubmitter for SupervisorWarmPromptSubmitter {
    fn submit_warm_prompt(
        &self,
        target: SessionRef,
        run: WarmRun,
        input_id: InputId,
    ) -> WarmPromptFuture<'_> {
        Box::pin(async move {
            let (Some(supervisor), Some(store)) = (self.supervisor.upgrade(), self.store.upgrade())
            else {
                return Err(WarmPromptRefused);
            };
            let record = match store.lock().await.session_record(&target).await {
                Ok(Some(record)) => record,
                Ok(None) | Err(_) => return Err(WarmPromptRefused),
            };
            let content = session_event_model::PromptContent::text(WARM_PROMPT_TEXT.to_owned())
                .map_err(|_| WarmPromptRefused)?;
            let request = ProviderPromptContentsRequest {
                operation_id: OperationId::generate(),
                input_id,
                target,
                requested_by: record.created_by,
                approver: record.approver,
                input: acp_client_runtime::ProviderTurnInput::Prompt(
                    session_event_model::ProviderSubmission::plain(vec![content]),
                ),
                busy: PromptBusyPolicy::RefuseBusy,
            };
            match supervisor.submit_warm_owner_prompt(request, run).await {
                Ok(InputAcceptance::Started) => Ok(()),
                _ => Err(WarmPromptRefused),
            }
        })
    }
}
