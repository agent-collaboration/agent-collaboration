//! Discovery, redemption and publication remain one cancellable future owned by the Host.
use super::{
    device_code_flow::{
        DeviceCodeFailure, DeviceLoginRequest, IssuedMachineToken, IssuerFailure,
        MachineTokenIssuer, RefreshCredentialBundle, run_device_code_login,
    },
    issuer_connection::MachineIssuerConnection,
    login_clock::FleetLoginClock,
    login_contract::{FleetLoginEvent, FleetLoginRefusal, RefreshCredentialKeeping},
    login_publication::MachineLoginPublication,
    token_renewal::{MachineRenewalConnection, renew_machine_token},
};
use codex_router_fleet_state::fleet_login::{FleetLoginCandidate, SavedFleetLogin, SignInAddress};
use fleet_identity_protocol::MachineEnvironment;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone)]
pub(super) struct MachineLoginWork {
    pub issuer: Arc<dyn MachineIssuerConnection>,
    pub clock: Arc<dyn FleetLoginClock>,
    pub publication: Arc<MachineLoginPublication>,
}
pub(super) struct ConfirmedLogin {
    pub saved: SavedFleetLogin,
    pub issued: IssuedMachineToken,
    pub issuer: Arc<dyn MachineTokenIssuer>,
}
pub(super) struct RefreshedLogin {
    pub issued: IssuedMachineToken,
    pub issuer: Arc<dyn MachineTokenIssuer>,
}
pub(super) enum IssuerSource {
    Discover(SignInAddress),
    Connected(Arc<dyn MachineTokenIssuer>),
}

impl MachineLoginWork {
    pub(super) async fn login(
        self,
        request: DeviceLoginRequest,
        sign_in: SignInAddress,
        machine_environment: Option<MachineEnvironment>,
        keeping: RefreshCredentialKeeping,
        codes: mpsc::UnboundedSender<FleetLoginEvent>,
    ) -> Result<ConfirmedLogin, DeviceCodeFailure> {
        let issuer = self.issuer.connect(&sign_in).await.map_err(|failure| {
            DeviceCodeFailure::Refused(match failure {
                IssuerFailure::InvalidResponse => FleetLoginRefusal::InvalidIssuerResponse {},
                _ => FleetLoginRefusal::IssuerUnavailable {},
            })
        })?;
        let issued = run_device_code_login(
            issuer.as_ref(),
            self.clock.as_ref(),
            &request,
            keeping,
            |code| {
                let _sent = codes.send(code);
            },
        )
        .await?;
        let candidate = FleetLoginCandidate {
            sign_in,
            organization_id: request.organization_id,
            enrollment_id: issued.token.credential.machine_client_id.clone(),
            requested_machine_environment: machine_environment,
            central: issued.central.into(),
        };
        let saved = self
            .publication
            .publish_login(&candidate, &issued.token.credential, keeping)
            .await
            .map_err(DeviceCodeFailure::OutcomeUnknown)?;
        Ok(ConfirmedLogin {
            saved,
            issued: issued.token,
            issuer,
        })
    }
    pub(super) async fn refresh(
        self,
        source: IssuerSource,
        credential: RefreshCredentialBundle,
        connection: Option<MachineRenewalConnection>,
    ) -> Result<RefreshedLogin, ()> {
        let issuer = match source {
            IssuerSource::Connected(issuer) => issuer,
            IssuerSource::Discover(sign_in) => {
                self.issuer.connect(&sign_in).await.map_err(|_| ())?
            }
        };
        let issued =
            renew_machine_token(issuer.clone(), self.publication, credential, connection).await?;
        Ok(RefreshedLogin { issued, issuer })
    }
}
