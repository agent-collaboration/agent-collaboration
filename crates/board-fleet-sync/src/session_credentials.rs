use fleet_sync_session::ConnectFailure;
use reqwest::header::HeaderValue;

#[derive(Clone)]
pub struct AccessServiceToken {
    pub(crate) client_id: HeaderValue,
    pub(crate) client_secret: HeaderValue,
}

impl AccessServiceToken {
    pub fn new(client_id: &str, client_secret: &str) -> Result<Self, ConnectFailure> {
        Ok(Self {
            client_id: sensitive_header(client_id)?,
            client_secret: sensitive_header(client_secret)?,
        })
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
