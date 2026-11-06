//! Parse the declared Host identity without defaults or account-state discovery.
use board_staging_cutover::OwnerAuthentication;
use collaboration_protocol::UuidIdentity;
use fleet_sync_protocol::PeerId;
use std::{ffi::OsString, path::PathBuf};
use url::Url;

pub(super) enum ExecutionMode {
    Isolated,
    Production {
        target: Url,
        confirmation: message_board::CanonicalHash,
    },
}

impl ExecutionMode {
    pub fn apply(
        self,
        plan: board_staging_cutover::OfflineRunPlan,
    ) -> Result<board_staging_cutover::OfflineRunPlan, board_staging_cutover::OfflineHostError>
    {
        match self {
            Self::Isolated => Ok(plan),
            Self::Production {
                target,
                confirmation,
            } => plan.with_production_confirmation(&target, confirmation),
        }
    }
}

pub(super) enum CutoverCommand {
    Help,
    Inspect(InspectRequest),
    Preview {
        plan: PathBuf,
        production_target: Option<Url>,
    },
    Offline {
        plan: PathBuf,
        authentication: OwnerAuthentication,
        mode: ExecutionMode,
    },
    Claim {
        plan: PathBuf,
        authentication: OwnerAuthentication,
        mode: ExecutionMode,
    },
    Verify {
        plan: PathBuf,
        mode: ExecutionMode,
    },
    Canary {
        plan: PathBuf,
        mode: ExecutionMode,
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
            Some(command @ ("offline" | "claim" | "verify" | "canary")) => {
                return Self::parse_plan(command, arguments);
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

    fn parse_plan(
        command: &str,
        mut arguments: impl Iterator<Item = OsString>,
    ) -> Result<Self, ArgumentError> {
        let mut plan = None;
        let mut authentication = None;
        let mut production = false;
        let mut dry_run = false;
        let mut target = None;
        let mut confirmation = None;
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
                Some("--production") if !production => production = true,
                Some("--dry-run") if !dry_run => dry_run = true,
                Some("--target") if target.is_none() => {
                    let value = arguments.next().ok_or(ArgumentError)?;
                    target = Some(
                        Url::parse(value.to_str().ok_or(ArgumentError)?)
                            .map_err(|_| ArgumentError)?,
                    );
                }
                Some("--confirm-production") if confirmation.is_none() => {
                    let value = arguments.next().ok_or(ArgumentError)?;
                    confirmation = Some(
                        value
                            .into_string()
                            .map_err(|_| ArgumentError)?
                            .try_into()
                            .map_err(|_| ArgumentError)?,
                    );
                }
                _ => return Err(ArgumentError),
            }
        }
        let plan = plan.ok_or(ArgumentError)?;
        if !production && (target.is_some() || confirmation.is_some()) {
            return Err(ArgumentError);
        }
        if dry_run {
            if confirmation.is_some() || (production && target.is_none()) {
                return Err(ArgumentError);
            }
            return Ok(Self::Preview {
                plan,
                production_target: target,
            });
        }
        let mode = if production {
            ExecutionMode::Production {
                target: target.ok_or(ArgumentError)?,
                confirmation: confirmation.ok_or(ArgumentError)?,
            }
        } else {
            ExecutionMode::Isolated
        };
        match command {
            "offline" => Ok(Self::Offline {
                plan,
                authentication: authentication.ok_or(ArgumentError)?,
                mode,
            }),
            "claim" => Ok(Self::Claim {
                plan,
                authentication: authentication.ok_or(ArgumentError)?,
                mode,
            }),
            "verify" if authentication.is_none() => Ok(Self::Verify { plan, mode }),
            "canary" if authentication.is_none() => Ok(Self::Canary { plan, mode }),
            _ => Err(ArgumentError),
        }
    }
}
