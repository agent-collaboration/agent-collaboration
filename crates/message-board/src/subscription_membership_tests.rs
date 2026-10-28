//! A Router may attest expiry, while every other membership change needs its declared requester.
use crate::*;

fn router_expiry_props(reason: Option<EndReason>) -> SubscriptionMembershipProps {
    let project = ProjectId::generate();
    let reader = Identity::Human {
        human_id: HumanId::try_from("membership-reader".to_owned()).unwrap(),
    };
    let service = ServiceId::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned()).unwrap();
    let cursor = FeedCursor {
        epoch: EpochNumber::try_from(1).unwrap(),
        position: FeedPosition::try_from(2).unwrap(),
    };
    let now = chrono::Utc::now();
    SubscriptionMembershipProps {
        envelope: RecordEnvelope {
            id: RecordId::generate(),
            scope: RecordScope {
                workspace: WorkspaceId::generate(),
                board: Some(BoardId::generate()),
                project: Some(project.clone()),
            },
            actor: RecordedActor::Router {
                component: RouterComponent::SubscriptionDelivery,
                installation: service.clone(),
            },
            request: None,
            position: RecordPlacement::Project(ProjectFeedEntry {
                cursor,
                origin: ActorOrigin {
                    installation: service,
                    machine: MachineLabel::try_from("membership fixture".to_owned()).unwrap(),
                },
            }),
            time: RecordTime::Router(now),
        },
        reader: reader.clone(),
        scope: SubscriptionScope::project(project),
        policy: SubscriptionPolicy::defaults_for(&reader),
        renewed_at: now - chrono::Duration::days(1),
        expires_at: now,
        generation: SubscriptionGeneration::new(1).unwrap(),
        activated_at: FeedCursor {
            epoch: cursor.epoch,
            position: FeedPosition::ZERO,
        },
        ended: reason.map(|reason| MembershipEnd {
            reason,
            position: cursor,
            at: now,
        }),
    }
}

#[test]
fn router_expiry_membership_constructs_and_decodes_with_its_original_reader() {
    let props = router_expiry_props(Some(EndReason::Expired));
    let expected_reader = props.reader.clone();
    let member =
        SubscriptionMembership::new(props).expect("Router-authored Expired membership is bound");
    assert_eq!(member.reader(), &expected_reader);
    let decoded: SubscriptionMembership =
        serde_json::from_value(serde_json::to_value(&member).unwrap()).unwrap();
    assert_eq!(decoded, member);
}

#[test]
fn router_non_expiry_membership_is_refused_on_construction_and_decode() {
    for reason in [
        None,
        Some(EndReason::Cancelled),
        Some(EndReason::Resolved),
        Some(EndReason::Left),
        Some(EndReason::Replaced),
    ] {
        let props = router_expiry_props(reason);
        let encoded = serde_json::to_value(&props).unwrap();
        assert_eq!(
            SubscriptionMembership::new(props).unwrap_err().field,
            "actor"
        );
        let error = serde_json::from_value::<SubscriptionMembership>(encoded).unwrap_err();
        assert!(error.to_string().contains("declared requester"));
    }
}

#[test]
fn a_missing_membership_requester_is_not_inferred_from_its_reader() {
    let mut encoded = serde_json::to_value(router_expiry_props(None)).unwrap();
    encoded
        .get_mut("envelope")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("actor");
    let error = serde_json::from_value::<SubscriptionMembership>(encoded).unwrap_err();
    assert!(error.to_string().contains("actor"));
}
