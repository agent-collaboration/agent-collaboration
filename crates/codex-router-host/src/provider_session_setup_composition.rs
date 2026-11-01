//! Adapt the client-owned initial-setup seam onto the sole registrar lifecycle owner.
use acp_client_runtime::{
    ProviderSessionCreateSetup, ProviderSessionSetupFuture, ProviderSessionSetupPort,
    ProviderSessionSetupReadiness, ProviderSessionSetupUnavailable, ProviderSettingKind,
};
use collaboration_contracts::{
    HarnessLaunchFailure, HarnessLocalSetupRefusal, HarnessRegistrationOutcome,
    PreparedHarnessSessionStart,
};
use collaboration_protocol::ProviderSettingName;
use message_board::SessionRef;
use std::sync::Arc;

/// Consume dispatch admission once, immediately before sending the provider's create.
/// The Host checks HTTP support first; a known create refusal abandons through the same port.
pub async fn prepare_provider_session_setup(
    start: PreparedHarnessSessionStart,
) -> Result<ProviderSessionCreateSetup, HarnessLaunchFailure> {
    start.port.mark_dispatched(&start.operation_id).await?;
    Ok(ProviderSessionCreateSetup::Managed {
        server_name: start.binding.server_name().to_owned(),
        server_url: start.binding.expose_address().to_owned(),
        port: Arc::new(HostProviderSessionSetup { start }),
    })
}

struct HostProviderSessionSetup {
    start: PreparedHarnessSessionStart,
}
impl ProviderSessionSetupPort for HostProviderSessionSetup {
    fn created<'a>(&'a self, provider_session_id: &'a str) -> ProviderSessionSetupFuture<'a, ()> {
        Box::pin(async move {
            let session = SessionRef {
                endpoint: self.start.endpoint.clone(),
                session_id: provider_session_id
                    .to_owned()
                    .try_into()
                    .map_err(|_| ProviderSessionSetupUnavailable)?,
            };
            self.start
                .port
                .record_created(&self.start.operation_id, &session)
                .await
                .map_err(|_| ProviderSessionSetupUnavailable)
        })
    }
    fn setup_passed<'a>(
        &'a self,
        _provider_session_id: &'a str,
    ) -> ProviderSessionSetupFuture<'a, ProviderSessionSetupReadiness> {
        Box::pin(async move {
            match self
                .start
                .port
                .complete_setup(&self.start.operation_id)
                .await
                .map_err(|_| ProviderSessionSetupUnavailable)?
            {
                HarnessRegistrationOutcome::Registered => Ok(ProviderSessionSetupReadiness::Ready),
                HarnessRegistrationOutcome::Pending => Ok(ProviderSessionSetupReadiness::Pending),
                HarnessRegistrationOutcome::Refused(_) => {
                    Ok(ProviderSessionSetupReadiness::Refused)
                }
            }
        })
    }
    fn setup_refused<'a>(
        &'a self,
        _provider_session_id: &'a str,
        setting: ProviderSettingKind,
    ) -> ProviderSessionSetupFuture<'a, ()> {
        Box::pin(async move {
            let setting = match setting {
                ProviderSettingKind::Mode => ProviderSettingName::Mode,
                ProviderSettingKind::Model => ProviderSettingName::Model,
                ProviderSettingKind::Effort => ProviderSettingName::Effort,
            };
            self.start
                .port
                .refuse_setup(
                    &self.start.operation_id,
                    HarnessLocalSetupRefusal::InitialSettingRefused { setting },
                )
                .await
                .map_err(|_| ProviderSessionSetupUnavailable)
        })
    }
}
