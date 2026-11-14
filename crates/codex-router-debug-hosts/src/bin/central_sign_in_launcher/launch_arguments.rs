//! The historical ten flags and their CENTRAL_LAUNCHER_<FLAG> fallbacks.
use super::launcher_failure::{LauncherFailure, LauncherResult};
use collaboration_central_core::SignInTrustSettings;
use fleet_identity_protocol::OrganizationId;
use runtime_primitives::SyncTime;
use std::{collections::BTreeMap, path::PathBuf};

const FLAGS: [&str; 10] = [
    "root",
    "socket",
    "issuer",
    "audience",
    "admin-audience",
    "public-origin",
    "jwks-url",
    "service-id",
    "legacy-organization",
    "fixed-time-ms",
];

struct LauncherArguments {
    values: BTreeMap<&'static str, String>,
}
impl LauncherArguments {
    fn parse() -> LauncherResult<Self> {
        let mut values = BTreeMap::new();
        let mut arguments = std::env::args().skip(1);
        while let Some(flag) = arguments.next() {
            let name = flag
                .strip_prefix("--")
                .ok_or(LauncherFailure::InvalidArguments)?;
            let known = FLAGS
                .iter()
                .find(|known| **known == name)
                .copied()
                .ok_or(LauncherFailure::InvalidArguments)?;
            let value = arguments
                .next()
                .ok_or(LauncherFailure::MissingValue(known))?;
            if values.insert(known, value).is_some() {
                return Err(LauncherFailure::DuplicateFlag(known));
            }
        }
        for name in FLAGS {
            if !values.contains_key(name) {
                let variable =
                    format!("CENTRAL_LAUNCHER_{}", name.to_uppercase().replace('-', "_"));
                if let Ok(value) = std::env::var(variable) {
                    values.insert(name, value);
                }
            }
        }
        Ok(Self { values })
    }
    fn get(&self, name: &'static str) -> LauncherResult<String> {
        self.values
            .get(name)
            .cloned()
            .ok_or(LauncherFailure::MissingValue(name))
    }
}

pub(super) struct LaunchConfiguration {
    pub(super) root: PathBuf,
    pub(super) socket: PathBuf,
    pub(super) trust: SignInTrustSettings,
    pub(super) fixed_time: SyncTime,
}
impl LaunchConfiguration {
    pub(super) fn from_process() -> LauncherResult<Self> {
        let arguments = LauncherArguments::parse()?;
        // Accepted for consumer compatibility; no legacy object or store is created.
        let _legacy: OrganizationId = arguments
            .get("legacy-organization")?
            .try_into()
            .map_err(|_| LauncherFailure::InvalidValue("legacy-organization"))?;
        let fixed_time = arguments
            .get("fixed-time-ms")?
            .parse::<i64>()
            .map_err(|_| LauncherFailure::InvalidValue("fixed-time-ms"))?;
        Ok(Self {
            root: arguments.get("root")?.into(),
            socket: arguments.get("socket")?.into(),
            trust: SignInTrustSettings {
                issuer: arguments.get("issuer")?,
                audience: arguments.get("audience")?,
                admin_audience: arguments.get("admin-audience")?,
                public_origin: arguments.get("public-origin")?,
                jwks_url: arguments.get("jwks-url")?,
                service_id: Some(
                    arguments
                        .get("service-id")?
                        .try_into()
                        .map_err(|_| LauncherFailure::InvalidValue("service-id"))?,
                ),
            },
            fixed_time: SyncTime::from_unix_millis(fixed_time),
        })
    }
}
