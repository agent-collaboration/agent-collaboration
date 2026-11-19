use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::RouteClaim;
use collaboration_contracts::RouteUnavailableReason;
use collaboration_protocol::{
    DeliveryNextAction, DeliveryPeerClaim, DeliveryRejection, DeliveryRejectionReason,
};
use serde_json::{json, to_value};

#[test]
fn route_claims_and_preconditions_keep_tagged_meaning() -> Result<(), Box<dyn std::error::Error>> {
    let unavailable = RouteClaim::Unavailable {
        reason: RouteUnavailableReason {
            reason: "provider process exited".into(),
            fix: "restart the debug Host".into(),
        },
        retryable: true,
    };
    let encoded = to_value(&unavailable)?;
    if encoded
        != json!({
            "kind":"unavailable","reason":{"reason":"provider process exited","fix":"restart the debug Host"},
            "retryable":true
        })
    {
        return Err("unavailable claim lost its reason or fix".into());
    }
    let decoded: RouteClaim = serde_json::from_value(encoded)?;
    if !matches!(
        decoded,
        RouteClaim::Unavailable {
            retryable: true,
            ..
        }
    ) {
        return Err("unavailable claim changed variant".into());
    }
    let rejected = RouteClaim::Rejected {
        rejection: DeliveryRejection {
            native_failure: None,
            reason: DeliveryRejectionReason::LiveElsewhere,
            next_action: DeliveryNextAction::InspectTarget,
            client_code: None,
            detail: Some("two live terminals claim this session".into()),
            claims: Some(vec![DeliveryPeerClaim {
                pid: 52304,
                name: None,
                cwd: None,
            }]),
        },
    };
    let encoded = to_value(&rejected)?;
    if encoded.pointer("/kind") != Some(&json!("rejected"))
        || encoded.pointer("/rejection/claims/0/pid") != Some(&json!(52304))
    {
        return Err("terminal rejection claim lost its typed payload".into());
    }
    let decoded: RouteClaim = serde_json::from_value(encoded)?;
    if !matches!(decoded, RouteClaim::Rejected { .. }) {
        return Err("terminal rejection claim changed variant".into());
    }
    let generation = json!({"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1});
    let decoded: DeliveryPrecondition = serde_json::from_value(json!({
        "kind":"endpointGeneration","expected":generation
    }))?;
    if !matches!(decoded, DeliveryPrecondition::EndpointGeneration { .. }) {
        return Err("strict generation guard changed variant".into());
    }
    if to_value(DeliveryPrecondition::Unpinned)? != json!({"kind":"unpinned"}) {
        return Err("unpinned guard changed shape".into());
    }
    Ok(())
}
