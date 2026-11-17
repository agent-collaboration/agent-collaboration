//! Deployment trust shared by the native and Worker sign-in adapters.

/// Hosts supply these settings explicitly when constructing an organization's
/// verifier. Transport addresses and the organization's clock belong to the host.
#[derive(Clone)]
pub struct SignInTrustSettings {
    pub issuer: String,
    /// Audience of organization stamps for this central deployment.
    pub audience: String,
    /// Audience of person tokens for central's admin surface.
    pub admin_audience: String,
    /// Byte-exact audience of machine access tokens for this deployment.
    pub machine_audience: String,
    /// Central's external HTTPS origin, used for DPoP request targets.
    pub public_origin: String,
    pub jwks_url: String,
    /// The sign-in service's identity on records written for a person.
    /// Admin calls refuse when the deployment has not configured this identity.
    pub service_id: Option<message_board::ServiceId>,
}

impl SignInTrustSettings {
    /// Validate D08's original address bytes; never rewrite the configured audience.
    #[must_use]
    pub fn machine_audience_is_valid(&self) -> bool {
        let value = &self.machine_audience;
        if value.is_empty()
            || value.len() > 2048
            || !value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        {
            return false;
        }
        let Some((scheme, address)) = value.split_once("://") else {
            return false;
        };
        if scheme != "https" && scheme != "http" {
            return false;
        }
        let (authority, path) = address
            .split_once('/')
            .map_or((address, ""), |(authority, path)| (authority, path));
        let (host, port) = if let Some(rest) = authority.strip_prefix("[::1]") {
            if rest.is_empty() {
                ("[::1]", None)
            } else if let Some(port) = rest.strip_prefix(':') {
                ("[::1]", Some(port))
            } else {
                return false;
            }
        } else {
            let (host, port) = authority
                .split_once(':')
                .map_or((authority, None), |(host, port)| (host, Some(port)));
            if host.is_empty()
                || !host.split('.').all(|label| {
                    let bytes = label.as_bytes();
                    !bytes.is_empty()
                        && bytes.len() <= 63
                        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
                        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
                        && bytes.iter().all(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-'
                        })
                })
            {
                return false;
            }
            (host, port)
        };
        if port.is_some_and(|port| {
            port.starts_with('0')
                || port.len() > 5
                || !port.bytes().all(|byte| byte.is_ascii_digit())
                || port.parse::<u16>().ok().is_none_or(|port| port == 0)
        }) {
            return false;
        }
        if scheme == "http" && !["localhost", "127.0.0.1", "[::1]"].contains(&host) {
            return false;
        }
        let mut bytes = path.bytes();
        while let Some(byte) = bytes.next() {
            if byte == b'%' {
                if !bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit())
                    || !bytes.next().is_some_and(|byte| byte.is_ascii_hexdigit())
                {
                    return false;
                }
            } else if !byte.is_ascii_alphanumeric() && !b"-._~!$&'()*+,;=:@/".contains(&byte) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
#[path = "sign_in_trust_settings_test.rs"]
mod tests;
