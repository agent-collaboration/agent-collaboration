use fleet_sync_session::ConnectFailure;
use reqwest::header::HeaderValue;

#[derive(Clone)]
pub enum AccessServiceToken {
    Cloudflare {
        client_id: HeaderValue,
        client_secret: HeaderValue,
    },
    SelfHosted {
        assertion: HeaderValue,
    },
}

impl AccessServiceToken {
    pub fn new(client_id: &str, client_secret: &str) -> Result<Self, ConnectFailure> {
        Ok(Self::Cloudflare {
            client_id: sensitive_header(client_id)?,
            client_secret: sensitive_header(client_secret)?,
        })
    }
    pub fn self_hosted(assertion: &str) -> Result<Self, ConnectFailure> {
        Ok(Self::SelfHosted {
            assertion: sensitive_header(assertion)?,
        })
    }
    pub(crate) fn apply(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self {
            Self::Cloudflare {
                client_id,
                client_secret,
            } => request
                .header("cf-access-client-id", client_id.clone())
                .header("cf-access-client-secret", client_secret.clone()),
            Self::SelfHosted { assertion } => {
                request.header("Cf-Access-Jwt-Assertion", assertion.clone())
            }
        }
    }
}

impl std::fmt::Debug for AccessServiceToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AccessServiceToken([redacted])")
    }
}

pub(crate) fn sensitive_header(value: &str) -> Result<HeaderValue, ConnectFailure> {
    if value.is_empty() {
        return Err(ConnectFailure::InvalidCredential);
    }
    let mut header = HeaderValue::from_str(value).map_err(|_| ConnectFailure::InvalidCredential)?;
    header.set_sensitive(true);
    Ok(header)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn cloudflare_headers_remain_exact_and_self_hosted_uses_only_the_assertion_header() {
        let request = reqwest::Client::new().get("http://127.0.0.1:1/v1/session");
        let cloudflare = AccessServiceToken::new("fixture-id", "fixture-secret").unwrap();
        let request = cloudflare.apply(request).build().unwrap();
        assert_eq!(request.headers().len(), 2);
        assert_eq!(request.headers()["cf-access-client-id"], "fixture-id");
        assert_eq!(
            request.headers()["cf-access-client-secret"],
            "fixture-secret"
        );
        let access = AccessServiceToken::self_hosted("private.fixture.assertion").unwrap();
        let request = access
            .apply(reqwest::Client::new().get("http://127.0.0.1:1/v1/session"))
            .build()
            .unwrap();
        assert_eq!(request.headers().len(), 1);
        assert_eq!(
            request.headers()["Cf-Access-Jwt-Assertion"],
            "private.fixture.assertion"
        );
        assert!(request.headers()["Cf-Access-Jwt-Assertion"].is_sensitive());
        assert_eq!(format!("{access:?}"), "AccessServiceToken([redacted])");
    }
}
