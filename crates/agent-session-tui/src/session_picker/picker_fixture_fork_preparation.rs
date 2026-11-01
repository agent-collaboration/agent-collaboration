//! Debug fixture material never enters the production composition or release build.
use super::{PickerFixtureScenario, PickerSourceContext, SessionsPickerRequest};
use crate::session_picker::source_action_worker::SourceActionPreparer;
use agent_session_inventory::sessions::{
    SessionPickerIdentity,
    router_connection_registry::{
        AbsoluteRemoteCwd, RouterConnectionProfile, RouterConnectionTransport,
    },
    source_ssh_protocol::{
        PreparedSourceLaunch, SourceReadError, SourceRejection, SourceSessionAction,
    },
};
use collaboration_client::protocol::SessionRef;
use std::sync::Arc;

#[derive(Clone)]
struct FixtureForkPreparation {
    profile: RouterConnectionProfile,
    target: SessionRef,
    directory: AbsoluteRemoteCwd,
}

pub fn source_preparer(
    scenario: Option<PickerFixtureScenario>,
    request: &SessionsPickerRequest,
) -> Option<SourceActionPreparer> {
    if scenario != Some(PickerFixtureScenario::ForkSshProfile) {
        return None;
    }
    let record = request.records.first()?;
    let SessionPickerIdentity::HostedCodex(target) = &record.identity else {
        return None;
    };
    let Some(PickerSourceContext::ConfiguredHosted(profile)) = &record.source_context else {
        return None;
    };
    let fixture = FixtureForkPreparation {
        profile: profile.clone(),
        target: target.clone(),
        directory: record.cwd.clone()?.try_into().ok()?,
    };
    Some(Arc::new(move |profile, action| {
        let fixture = fixture.clone();
        Box::pin(async move { fixture.prepare(profile, action) })
    }))
}

impl FixtureForkPreparation {
    fn prepare(
        self,
        profile: RouterConnectionProfile,
        action: SourceSessionAction,
    ) -> Result<PreparedSourceLaunch, SourceReadError> {
        let RouterConnectionTransport::Ssh(connection) = &profile.transport else {
            return Err(SourceReadError::Rejected(SourceRejection::InvalidRequest));
        };
        if profile != self.profile
            || !matches!(&action,SourceSessionAction::Fork{target,..} if *target==self.target)
        {
            return Err(SourceReadError::Rejected(SourceRejection::InvalidRequest));
        }
        Ok(PreparedSourceLaunch {
            action,
            endpoint:self.target.endpoint,
            generation:serde_json::from_value(serde_json::json!({"serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":1})).map_err(|_|SourceReadError::InvalidReply)?,
            schema_digest:format!("sha256:{}","a".repeat(64)).try_into().map_err(|_|SourceReadError::InvalidReply)?,
            working_directory:self.directory,
            client_profile:connection.client_profile,
        })
    }
}
