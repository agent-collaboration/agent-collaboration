//! Prepare and verify native attachment from the existing recorded Router policy.
use super::{RouterSessionProfile, SessionSetupError, normalized_directory};
use crate::ApprovalRoute;
use collaboration_protocol::PermissionDifference;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Opaque projection shared by native callers; the ACP profile owner defines the grants.
pub struct RecordedSessionResume {
    thread_id: String,
    working_directory: PathBuf,
    profile: RouterSessionProfile,
}

impl RecordedSessionResume {
    pub async fn prepare(
        route: &ApprovalRoute,
        thread_id: &str,
        working_directory: &str,
    ) -> Result<Self, SessionSetupError> {
        if route.thread_id != thread_id
            || thread_id.is_empty()
            || !Path::new(&route.scratch_path).is_absolute()
            || route.scratch_path.contains('\0')
        {
            return Err(SessionSetupError::ConfigurationMismatch);
        }
        let working_directory = normalized_directory(working_directory).await?;
        let profile = RouterSessionProfile::for_session(
            route.access,
            &working_directory,
            Path::new(&route.scratch_path),
        )
        .await?;
        Ok(Self {
            thread_id: thread_id.to_owned(),
            working_directory,
            profile,
        })
    }

    #[must_use]
    pub fn native_parameters(&self) -> Value {
        json!({
            "threadId":self.thread_id,
            "permissions":self.profile.name(),
            "config":self.profile.native_config(),
            "excludeTurns":true
        })
    }

    /// Rejoin loaded state without overrides that could make native rebuild its cache.
    /// A concurrent unload can still produce a cold response; validate it before input.
    #[must_use]
    pub fn native_parameters_for_loaded_thread(&self) -> Value {
        json!({"threadId":self.thread_id,"excludeTurns":true})
    }

    /// Validate thread identity and cwd; permission differences are report-only.
    pub async fn validate_response(
        &self,
        response: &Value,
    ) -> Result<Vec<PermissionDifference>, SessionSetupError> {
        if response.pointer("/thread/id").and_then(Value::as_str) != Some(&self.thread_id) {
            return Err(SessionSetupError::OutcomeUnknown);
        }
        for pointer in ["/cwd", "/thread/cwd"] {
            let directory = response
                .pointer(pointer)
                .and_then(Value::as_str)
                .ok_or(SessionSetupError::OutcomeUnknown)?;
            if normalized_directory(directory).await? != self.working_directory {
                return Err(SessionSetupError::ConfigurationMismatch);
            }
        }
        Ok(self.profile.observed_differences(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn permission_reporting_preserves_thread_and_both_cwd_checks() {
        // Arrange: a recorded route with a fixed thread and working directory.
        let actor = json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"creator"});
        let route: ApprovalRoute = serde_json::from_value(json!({
            "threadId":"recorded-thread","createdBy":actor,"approver":actor,
            "access":"workspace-write","scratchPath":"/recorded-scratch","rootMessageId":null
        }))
        .unwrap();
        let prepared =
            RecordedSessionResume::prepare(&route, "recorded-thread", "/recorded-project")
                .await
                .unwrap();
        let response = json!({"thread":{"id":"recorded-thread","cwd":"/recorded-project"},"cwd":"/recorded-project"});
        // Act/assert: missing permissions report every field and still allow input.
        assert_eq!(
            prepared.validate_response(&response).await.unwrap(),
            vec![
                PermissionDifference::Profile,
                PermissionDifference::Sandbox,
                PermissionDifference::WritableRoots,
                PermissionDifference::Network,
            ]
        );
        for pointer in ["/cwd", "/thread/cwd"] {
            let mut wrong_cwd = response.clone();
            *wrong_cwd.pointer_mut(pointer).unwrap() = json!("/other-project");
            assert!(matches!(
                prepared.validate_response(&wrong_cwd).await,
                Err(SessionSetupError::ConfigurationMismatch)
            ));
        }
        let mut wrong_thread = response;
        wrong_thread["thread"]["id"] = json!("other-thread");
        assert!(matches!(
            prepared.validate_response(&wrong_thread).await,
            Err(SessionSetupError::OutcomeUnknown)
        ));
    }
}
