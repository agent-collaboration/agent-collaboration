use crate::object_rpc::RpcIdentity;
use collaboration_central_core::*;
pub(crate) fn authorized(
    caller_kind: CallerKind,
    caller: &RpcIdentity,
    address: &ObjectAddress,
    input: &WireInput,
) -> Result<(), RpcRefusal> {
    let WireInput::Call { call, .. } = input else {
        return Err(RpcRefusal::CallerClassRefused);
    };
    let target = call.kind();
    let coherent = match address {
        ObjectAddress::Connection(_) => {
            matches!(target, CallerKind::Session | CallerKind::Delivery)
        }
        address => target == address.primary_face(),
    };
    if !coherent {
        return Err(RpcRefusal::CallerClassRefused);
    }
    validate_call_edge(caller_kind, call.kind()).map_err(RpcRefusal::CallEdgeRefused)?;
    if authorized_identity(caller_kind, caller, address, input) {
        Ok(())
    } else {
        Err(RpcRefusal::CallerClassRefused)
    }
}
fn authorized_identity(
    caller_kind: CallerKind,
    caller: &RpcIdentity,
    address: &ObjectAddress,
    input: &WireInput,
) -> bool {
    #[cfg(feature = "conformance")]
    if matches!(caller, RpcIdentity::Conformance) {
        return true;
    }
    if !matches!(caller, RpcIdentity::Object(_)) && caller_kind != CallerKind::Edge {
        return false;
    }
    let WireInput::Call { from, call, .. } = input else {
        return false;
    };
    if let RpcIdentity::Object(source) = caller
        && source != from
    {
        return false;
    }
    if let RpcIdentity::Object(source) = caller {
        let coherent = match source {
            ObjectAddress::Connection(_) => {
                matches!(caller_kind, CallerKind::Session | CallerKind::Delivery)
            }
            address => caller_kind == address.primary_face(),
        };
        if !coherent {
            return false;
        }
    }
    match (caller, address, call.as_ref()) {
        (RpcIdentity::Admin, ObjectAddress::Fleet, WireCall::Fleet(FleetCall::Import { .. })) => {
            true
        }
        (RpcIdentity::Admin, ObjectAddress::Copy(_), WireCall::Copy(CopyCall::Ops(_))) => true,
        (
            RpcIdentity::Admin,
            ObjectAddress::Copy(_),
            WireCall::Copy(CopyCall::ImportSeed { .. }),
        ) => true,
        (
            RpcIdentity::Admin,
            ObjectAddress::Registry,
            WireCall::Registry(RegistryCall::RecordUnheld(_)),
        ) => true,
        (
            RpcIdentity::Object(ObjectAddress::Connection(sender)),
            ObjectAddress::Registry,
            WireCall::Registry(RegistryCall::Claim { claimant, .. }),
        ) => sender == claimant,
        (
            RpcIdentity::Object(ObjectAddress::Registry),
            ObjectAddress::Copy(_),
            WireCall::Copy(CopyCall::ImportSeed { .. }),
        ) => true,
        (
            RpcIdentity::Object(ObjectAddress::Registry),
            ObjectAddress::Fleet,
            WireCall::Fleet(
                FleetCall::DirectoryUpdate(_)
                | FleetCall::ReserveClaim { .. }
                | FleetCall::ReleaseClaim { .. }
                | FleetCall::AdvanceLeaseIncarnation { .. },
            ),
        ) => true,
        (
            RpcIdentity::Object(ObjectAddress::Copy(source)),
            ObjectAddress::Fleet,
            WireCall::Fleet(FleetCall::DirectoryUpdate(DirectoryUpdate::Summary {
                project, ..
            })),
        ) => source == project,
        (
            RpcIdentity::Object(ObjectAddress::Connection(sender)),
            ObjectAddress::Registry,
            WireCall::Registry(RegistryCall::ForwardRoute {
                sender: claimed, ..
            }),
        ) => sender == claimed,
        (
            RpcIdentity::Object(ObjectAddress::Connection(sender)),
            ObjectAddress::Connection(_),
            WireCall::Delivery(DeliveryCall::DeliverForward { origin, .. }),
        ) => sender == origin,
        (
            RpcIdentity::Object(ObjectAddress::Connection(_)),
            ObjectAddress::Connection(_),
            WireCall::Delivery(DeliveryCall::ForwardAnswered { .. }),
        ) => true,
        (
            RpcIdentity::Edge,
            ObjectAddress::Registry,
            WireCall::Registry(RegistryCall::AuthenticateMachine { .. }),
        ) => true,
        (
            RpcIdentity::Admin,
            ObjectAddress::Registry,
            WireCall::Registry(RegistryCall::Admin {
                authority: AdminAuthority::Admin { .. },
                ..
            }),
        ) => true,
        (
            RpcIdentity::Object(ObjectAddress::Connection(sender)),
            ObjectAddress::Registry,
            WireCall::Registry(RegistryCall::Renew { installation, .. })
            | WireCall::Registry(RegistryCall::VerifyTail { installation, .. })
            | WireCall::Registry(RegistryCall::Release { installation, .. })
            | WireCall::Registry(RegistryCall::SessionDisconnected { installation, .. }),
        ) => sender == installation,
        (
            RpcIdentity::Object(ObjectAddress::Connection(sender)),
            ObjectAddress::Registry,
            WireCall::Registry(RegistryCall::RegisterSession(registration)),
        ) => sender == &registration.installation,
        (
            RpcIdentity::Object(ObjectAddress::Connection(sender)),
            ObjectAddress::Registry,
            WireCall::Registry(RegistryCall::ReserveFirstEpoch {
                source: ReservationSource::Machine { installation },
                ..
            })
            | WireCall::Registry(RegistryCall::ReserveFirstEpoch {
                source: ReservationSource::FleetAcceptance { installation },
                ..
            }),
        ) => sender == installation,
        (
            RpcIdentity::Object(ObjectAddress::Connection(sender)),
            ObjectAddress::Copy(_),
            WireCall::Copy(CopyCall::ApplyPiece {
                sender: installation,
                ..
            }),
        ) => sender == installation,
        (
            RpcIdentity::Object(ObjectAddress::Connection(_) | ObjectAddress::Registry),
            ObjectAddress::Copy(_),
            WireCall::Copy(CopyCall::CopyStatus { .. })
            | WireCall::Copy(CopyCall::SnapshotPage { .. }),
        ) => true,
        (
            RpcIdentity::Object(ObjectAddress::Registry),
            ObjectAddress::Copy(_),
            WireCall::Copy(CopyCall::Initialize { .. }) | WireCall::Copy(CopyCall::Fence { .. }),
        ) => true,
        (
            RpcIdentity::Object(ObjectAddress::Registry | ObjectAddress::Fleet),
            ObjectAddress::Connection(_),
            WireCall::Delivery(DeliveryCall::CloseSession { .. })
            | WireCall::Delivery(DeliveryCall::DeliverNotice(_)),
        ) => true,
        (
            RpcIdentity::Object(ObjectAddress::Connection(sender)),
            ObjectAddress::Fleet,
            WireCall::Fleet(FleetCall::Fleet { installation, .. })
            | WireCall::Fleet(FleetCall::Read { installation, .. })
            | WireCall::Fleet(FleetCall::ArchiveSeen { installation, .. }),
        ) => sender == installation,
        (
            RpcIdentity::Object(ObjectAddress::Connection(_)),
            ObjectAddress::Fleet,
            WireCall::Fleet(FleetCall::StoreTail { .. }),
        ) => true,
        _ => false,
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum RpcRefusal {
    CallEdgeRefused(CallEdgeRefused),
    CallerClassRefused,
}
