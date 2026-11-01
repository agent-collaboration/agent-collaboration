//! Each saved or requested sign-in address is discovered before this Host uses its OAuth endpoints.
use std::sync::Arc;

use codex_router_fleet_state::fleet_login::SignInAddress;

use super::{
    device_code_flow::{IssuerFailure, MachineTokenIssuer},
    issuer_discovery::discover_issuer,
    issuer_http::HttpMachineTokenIssuer,
    login_clock::{FleetLoginClock, LoginFuture},
};

pub(super) trait MachineIssuerConnection: Send + Sync {
    fn connect<'a>(
        &'a self,
        sign_in: &'a SignInAddress,
    ) -> LoginFuture<'a, Result<Arc<dyn MachineTokenIssuer>, IssuerFailure>>;
}

pub(super) struct HttpMachineIssuerConnection {
    pub client: reqwest::Client,
    pub clock: Arc<dyn FleetLoginClock>,
}
impl MachineIssuerConnection for HttpMachineIssuerConnection {
    fn connect<'a>(
        &'a self,
        sign_in: &'a SignInAddress,
    ) -> LoginFuture<'a, Result<Arc<dyn MachineTokenIssuer>, IssuerFailure>> {
        Box::pin(async move {
            let configuration = discover_issuer(&self.client, sign_in).await?;
            Ok(Arc::new(HttpMachineTokenIssuer::new(
                configuration,
                self.client.clone(),
                self.clock.clone(),
            )) as Arc<dyn MachineTokenIssuer>)
        })
    }
}
