//! The full persisted harness address is a bearer capability; diagnostics show no capability bytes.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SessionBindingOrigin(String);

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SessionBindingAddress(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid session binding address")]
pub struct InvalidSessionBindingAddress;

fn parse_address(value: &str) -> Result<url::Url, InvalidSessionBindingAddress> {
    let url = url::Url::parse(value).map_err(|_| InvalidSessionBindingAddress)?;
    if value.len() > 4096
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(InvalidSessionBindingAddress);
    }
    Ok(url)
}

impl TryFrom<String> for SessionBindingOrigin {
    type Error = InvalidSessionBindingAddress;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let url = parse_address(&value)?;
        if !matches!(url.path(), "" | "/") {
            return Err(InvalidSessionBindingAddress);
        }
        Ok(Self(url.origin().ascii_serialization()))
    }
}
impl SessionBindingOrigin {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<SessionBindingOrigin> for String {
    fn from(value: SessionBindingOrigin) -> Self {
        value.0
    }
}

impl TryFrom<String> for SessionBindingAddress {
    type Error = InvalidSessionBindingAddress;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let url = parse_address(&value)?;
        let capability = url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .ok_or(InvalidSessionBindingAddress)?;
        if capability.len() != 32
            || !capability
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(InvalidSessionBindingAddress);
        }
        Ok(Self(value))
    }
}
impl SessionBindingAddress {
    /// The harness needs this full address; it includes the bearer capability.
    #[must_use]
    pub fn expose_address(&self) -> &str {
        &self.0
    }
    pub fn origin(&self) -> Result<SessionBindingOrigin, InvalidSessionBindingAddress> {
        parse_address(&self.0)?
            .origin()
            .ascii_serialization()
            .try_into()
    }
}
impl From<SessionBindingAddress> for String {
    fn from(value: SessionBindingAddress) -> Self {
        value.0
    }
}
impl std::fmt::Debug for SessionBindingAddress {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SessionBindingAddress([REDACTED])")
    }
}
