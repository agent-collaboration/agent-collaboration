//! A single owner HTTP call. Pending and uncertain outcomes remain visible to callers.
use collaboration_central_admin_protocol::*;
use reqwest::{Client, Method, Request, Url, header::HeaderValue};
use serde::{Deserialize, de::DeserializeOwned};

pub enum CentralAdminOperation {
    Copies(CopyPageQuery),
    Health(DatabaseId),
    Repair(DatabaseId),
    Migrate(CopyPageQuery),
    Preview(DatabaseId),
    Move {
        database: DatabaseId,
        request: OwnerMove,
    },
    Takeover {
        database: DatabaseId,
        request: OwnerTakeover,
    },
    Retire {
        database: DatabaseId,
        request: OwnerRetirement,
    },
}
pub enum CentralAdminAnswer {
    Copies(CopyDirectoryPage),
    Health(CopyHealth),
    Repair(CopyRepairAnswer),
    Migrate(CopyMigrationPage),
    Preview(OwnerPreviewResponse),
    Transfer(OwnerTransferAnswer),
    Retirement(OwnerRetirementAnswer),
    Failure(OwnerOperationFailure),
    Unauthorized,
}
#[derive(Debug, thiserror::Error)]
pub enum CentralAdminError {
    #[error("--central must be an HTTP(S) URL without credentials, query or fragment")]
    InvalidCentralUrl,
    #[error("owner Access token cannot be used as an HTTP header")]
    InvalidOwnerToken,
    #[error("central request could not be prepared: {0}")]
    InvalidRequest(String),
    #[error("central read unavailable: {0}")]
    Unavailable(String),
    #[error(
        "central operation outcome is unknown; inspect its existing request ID before repeating: {0}"
    )]
    OutcomeUnknown(String),
}
/// The Access credential stays in memory and is marked sensitive to HTTP diagnostics.
pub struct CentralAdminClient {
    client: Client,
    central: Url,
    owner_access: HeaderValue,
}
impl CentralAdminClient {
    pub fn validate_url(central: &str) -> Result<Url, CentralAdminError> {
        let parsed = Url::parse(central).map_err(|_| CentralAdminError::InvalidCentralUrl)?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(CentralAdminError::InvalidCentralUrl);
        }
        Ok(parsed)
    }
    pub fn new(central: &str, owner_access: &str) -> Result<Self, CentralAdminError> {
        let central = Self::validate_url(central)?;
        let mut owner_access = HeaderValue::from_str(owner_access)
            .map_err(|_| CentralAdminError::InvalidOwnerToken)?;
        owner_access.set_sensitive(true);
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|error| CentralAdminError::Unavailable(error.to_string()))?;
        Ok(Self {
            client,
            central,
            owner_access,
        })
    }
    /// Build the exact production request without sending it or acquiring credentials.
    pub fn prepare(&self, operation: &CentralAdminOperation) -> Result<Request, CentralAdminError> {
        let (method, path) = match operation {
            CentralAdminOperation::Copies(_) => (Method::GET, "/admin/v1/copies".to_owned()),
            CentralAdminOperation::Health(database) => {
                (Method::GET, format!("/admin/v1/copies/{database}/health"))
            }
            CentralAdminOperation::Repair(database) => {
                (Method::POST, format!("/admin/v1/copies/{database}/repair"))
            }
            CentralAdminOperation::Migrate(_) => {
                (Method::POST, "/admin/v1/copies/migrate".to_owned())
            }
            CentralAdminOperation::Preview(database) => (
                Method::POST,
                format!("/admin/v1/projects/{database}/takeover/preview"),
            ),
            CentralAdminOperation::Move { database, .. } => {
                (Method::POST, format!("/admin/v1/projects/{database}/move"))
            }
            CentralAdminOperation::Takeover { database, .. } => (
                Method::POST,
                format!("/admin/v1/projects/{database}/takeover"),
            ),
            CentralAdminOperation::Retire { database, .. } => (
                Method::POST,
                format!("/admin/v1/projects/{database}/retire"),
            ),
        };
        let mut url = self.central.clone();
        url.set_path(&format!(
            "{}{path}",
            self.central.path().trim_end_matches('/')
        ));
        let request = self
            .client
            .request(method, url)
            .header("cf-access-token", self.owner_access.clone());
        let request = match operation {
            CentralAdminOperation::Copies(query) | CentralAdminOperation::Migrate(query) => {
                request.query(query)
            }
            CentralAdminOperation::Move { request: body, .. } => request.json(body),
            CentralAdminOperation::Takeover { request: body, .. } => request.json(body),
            CentralAdminOperation::Retire { request: body, .. } => request.json(body),
            CentralAdminOperation::Health(_)
            | CentralAdminOperation::Repair(_)
            | CentralAdminOperation::Preview(_) => request,
        };
        request
            .build()
            .map_err(|error| CentralAdminError::InvalidRequest(error.to_string()))
    }
    pub async fn execute(
        &self,
        operation: &CentralAdminOperation,
    ) -> Result<CentralAdminAnswer, CentralAdminError> {
        let request = self.prepare(operation)?;
        let mutating = request.method() != Method::GET;
        let uncertain = |detail: String| {
            if mutating {
                CentralAdminError::OutcomeUnknown(detail)
            } else {
                CentralAdminError::Unavailable(detail)
            }
        };
        let response = self
            .client
            .execute(request)
            .await
            .map_err(|error| uncertain(error.to_string()))?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Ok(CentralAdminAnswer::Unauthorized);
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| uncertain(error.to_string()))?;
        if status != reqwest::StatusCode::OK && status != reqwest::StatusCode::SERVICE_UNAVAILABLE {
            return serde_json::from_slice::<OwnerOperationFailure>(&bytes)
                .map(CentralAdminAnswer::Failure)
                .map_err(|_| uncertain(format!("HTTP {status} with no typed admin answer")));
        }
        let decoded = match operation {
            CentralAdminOperation::Copies(_) => decode(&bytes, CentralAdminAnswer::Copies),
            CentralAdminOperation::Health(_) => decode(&bytes, CentralAdminAnswer::Health),
            CentralAdminOperation::Repair(_) => decode(&bytes, CentralAdminAnswer::Repair),
            CentralAdminOperation::Migrate(_) => decode(&bytes, CentralAdminAnswer::Migrate),
            CentralAdminOperation::Preview(_) => decode(&bytes, CentralAdminAnswer::Preview),
            CentralAdminOperation::Move { .. } | CentralAdminOperation::Takeover { .. } => {
                decode(&bytes, CentralAdminAnswer::Transfer)
            }
            CentralAdminOperation::Retire { .. } => decode(&bytes, CentralAdminAnswer::Retirement),
        };
        let answer =
            decoded.map_err(|_| uncertain(format!("HTTP {status} with no typed admin answer")))?;
        if !answer_matches_http_status(status, &answer) {
            return Err(uncertain(format!(
                "HTTP {status} carried an inconsistent success body"
            )));
        }
        Ok(answer)
    }
}
fn answer_matches_http_status(status: reqwest::StatusCode, answer: &CentralAdminAnswer) -> bool {
    status == reqwest::StatusCode::OK
        || (status == reqwest::StatusCode::SERVICE_UNAVAILABLE
            && matches!(
                answer,
                CentralAdminAnswer::Failure(_)
                    | CentralAdminAnswer::Transfer(OwnerTransferAnswer::Unavailable { .. })
                    | CentralAdminAnswer::Retirement(OwnerRetirementAnswer::Unavailable { .. })
            ))
}
#[derive(Deserialize)]
#[serde(untagged)]
enum AdminReply<Answer> {
    Answer(Answer),
    Failure(OwnerOperationFailure),
}
fn decode<Answer: DeserializeOwned>(
    bytes: &[u8],
    map: impl FnOnce(Answer) -> CentralAdminAnswer,
) -> Result<CentralAdminAnswer, serde_json::Error> {
    match serde_json::from_slice::<AdminReply<Answer>>(bytes)? {
        AdminReply::Answer(answer) => Ok(map(answer)),
        AdminReply::Failure(failure) => Ok(CentralAdminAnswer::Failure(failure)),
    }
}

#[cfg(test)]
#[allow(clippy::panic_in_result_fn)]
mod tests {
    use super::*;
    #[test]
    fn http_error_cannot_masquerade_as_a_successful_migration_page() {
        let page = CentralAdminAnswer::Migrate(CopyMigrationPage {
            results: vec![],
            next_after: None,
        });
        assert!(!answer_matches_http_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            &page
        ));
        assert!(answer_matches_http_status(reqwest::StatusCode::OK, &page));
        assert!(answer_matches_http_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            &CentralAdminAnswer::Failure(OwnerOperationFailure::ObjectUnavailable)
        ));
    }
    #[test]
    fn typed_unavailable_keeps_its_subject_and_reason() -> Result<(), Box<dyn std::error::Error>> {
        let response=br#"{"kind":"unavailable","value":{"subject":{"kind":"transfer","transfer":"019f0000-0000-7000-8000-000000005881"},"reason":{"kind":"migrationPending","value":{"version":3}}}}"#;
        let CentralAdminAnswer::Transfer(OwnerTransferAnswer::Unavailable { subject, reason }) =
            decode(response, CentralAdminAnswer::Transfer)?
        else {
            return Err("typed503 became a generic success or failure".into());
        };
        assert_eq!(
            serde_json::to_value(subject)?,
            serde_json::json!({"kind":"transfer","transfer":"019f0000-0000-7000-8000-000000005881"})
        );
        assert_eq!(
            serde_json::to_value(reason)?,
            serde_json::json!({"kind":"migrationPending","value":{"version":3}})
        );
        let CentralAdminAnswer::Failure(OwnerOperationFailure::Retired) =
            decode::<CopyRepairAnswer>(br#"{"kind":"retired"}"#, CentralAdminAnswer::Repair)?
        else {
            return Err("typed410 was not preserved".into());
        };
        assert!(
            decode::<OwnerTransferAnswer>(
                br#"{"kind":"inventedSuccess"}"#,
                CentralAdminAnswer::Transfer
            )
            .is_err()
        );
        Ok(())
    }
}
