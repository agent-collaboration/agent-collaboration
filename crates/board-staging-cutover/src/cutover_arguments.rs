//! Parse the declared Host identity without defaults or account-state discovery.
use board_staging_cutover::OwnerAuthentication;
use collaboration_protocol::UuidIdentity;
use fleet_sync_protocol::PeerId;
use std::{ffi::OsString, path::PathBuf};

pub(super) enum CutoverCommand {
    Help,
    Inspect(InspectRequest),
    Offline {
        plan: PathBuf,
        authentication: OwnerAuthentication,
    },
    Claim {
        plan: PathBuf,
        authentication: OwnerAuthentication,
    },
}

pub(super) struct InspectRequest {
    pub router_root: PathBuf,
    pub service_id: UuidIdentity,
    pub peer_id: PeerId,
}

#[derive(Debug, thiserror::Error)]
#[error("invalid cutover arguments; use --help")]
pub(super) struct ArgumentError;

impl CutoverCommand {
    pub(super) fn parse(
        arguments: impl IntoIterator<Item = OsString>,
    ) -> Result<Self, ArgumentError> {
        let mut arguments = arguments.into_iter();
        match arguments.next().as_deref().and_then(|value| value.to_str()) {
            Some("--help" | "-h") if arguments.next().is_none() => return Ok(Self::Help),
            Some("inspect") => {}
            Some("offline") => {
                let (plan, authentication) = Self::parse_authenticated_plan(arguments)?;
                return Ok(Self::Offline {
                    plan,
                    authentication,
                });
            }
            Some("claim") => {
                let (plan, authentication) = Self::parse_authenticated_plan(arguments)?;
                return Ok(Self::Claim {
                    plan,
                    authentication,
                });
            }
            _ => return Err(ArgumentError),
        }
        let mut router_root = None;
        let mut service_id = None;
        let mut peer_id = None;
        while let Some(flag) = arguments.next() {
            let value = arguments.next().ok_or(ArgumentError)?;
            match flag.to_str() {
                Some("--router-root") if router_root.is_none() => {
                    router_root = Some(PathBuf::from(value));
                }
                Some("--service-id") if service_id.is_none() => {
                    service_id = Some(
                        value
                            .into_string()
                            .map_err(|_| ArgumentError)?
                            .try_into()
                            .map_err(|_| ArgumentError)?,
                    );
                }
                Some("--peer-id") if peer_id.is_none() => {
                    peer_id = Some(
                        value
                            .to_str()
                            .ok_or(ArgumentError)?
                            .parse()
                            .map_err(|_| ArgumentError)?,
                    );
                }
                _ => return Err(ArgumentError),
            }
        }
        Ok(Self::Inspect(InspectRequest {
            router_root: router_root.ok_or(ArgumentError)?,
            service_id: service_id.ok_or(ArgumentError)?,
            peer_id: peer_id.ok_or(ArgumentError)?,
        }))
    }

    fn parse_authenticated_plan(
        mut arguments: impl Iterator<Item = OsString>,
    ) -> Result<(PathBuf, OwnerAuthentication), ArgumentError> {
        let mut plan = None;
        let mut authentication = None;
        while let Some(flag) = arguments.next() {
            match flag.to_str() {
                Some("--plan") if plan.is_none() => {
                    plan = Some(PathBuf::from(arguments.next().ok_or(ArgumentError)?));
                }
                Some("--owner-credential-file") if authentication.is_none() => {
                    authentication = Some(OwnerAuthentication::OwnerCredentialFile {
                        path: PathBuf::from(arguments.next().ok_or(ArgumentError)?),
                    });
                }
                Some("--owner-access") if authentication.is_none() => {
                    authentication = Some(OwnerAuthentication::OwnerAccess)
                }
                _ => return Err(ArgumentError),
            }
        }
        Ok((
            plan.ok_or(ArgumentError)?,
            authentication.ok_or(ArgumentError)?,
        ))
    }
}
