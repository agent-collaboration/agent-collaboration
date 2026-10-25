//! Native activity keeps exact range holes while its header counts distinct threads.
use collaboration_protocol::{
    MachineId, MachineLabel, PushActivityRange, PushActivitySnapshot, PushDeliveryState,
    PushHeaderFacts, PushId, PushKind, PushLineInput, PushOrigin, PushRecord, PushRecordDraft,
    RouterLink, RouterOriginRef, SessionRef, render_push_line,
};
use serde_json::json;

#[allow(clippy::unwrap_used)] // Fixed native test literals fail at fixture construction.
fn activity_range(thread: u64, from: u64, through: u64) -> PushActivityRange {
    serde_json::from_value(json!({
        "threadId":format!("01900000-0000-7000-8000-{thread:012x}"),
        "projectId":"01900000-0000-7000-8000-000000000100",
        "from":{"epoch":1,"position":from},
        "through":{"epoch":1,"position":through}
    }))
    .unwrap()
}

#[allow(clippy::unwrap_used)] // This helper only constructs the permanent protocol fixtures.
fn draft(ranges: Vec<PushActivityRange>, root_count: u8) -> PushRecordDraft {
    let target: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
        "sessionId":"legacy-recipient"
    }))
    .unwrap();
    let push_id = PushId::try_from("01900000-0000-7000-8000-000000000200".to_owned()).unwrap();
    PushRecordDraft {
        push_id: push_id.clone(),
        kind: PushKind::SubscriptionActivity,
        origin: PushOrigin::Router(PushKind::SubscriptionActivity),
        origin_router_ref: Some(
            RouterOriginRef::SubscriptionActivity {
                target: target.clone(),
                batch_id: push_id.as_str().to_owned().try_into().unwrap(),
            }
            .canonical_string()
            .unwrap(),
        ),
        target,
        mode: None,
        guard: None,
        reply_to_push_id: None,
        header_facts: PushHeaderFacts::SubscriptionActivity {
            root_count,
            message_count: 7,
            held_since: None,
            thread_resolved: false,
        },
        body: None,
        activity: Some(PushActivitySnapshot {
            ranges,
            held: false,
            draining: false,
        }),
        created_at: chrono::DateTime::parse_from_rfc3339("2026-10-18T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
    }
}

#[test]
fn legacy_single_range_stored_shape_still_decodes_and_renders_the_same_line() {
    let original = draft(vec![activity_range(1, 2, 300)], 1);
    // Row decoding uses this validator after parsing the existing stored fields.
    let decoded: PushRecordDraft =
        serde_json::from_value(serde_json::to_value(original).unwrap()).unwrap();
    let record =
        PushRecord::from_storage_parts(decoded, PushDeliveryState::Pending, None, None, None, None)
            .unwrap();
    let line = render_push_line(&PushLineInput {
        link: RouterLink::new(
            MachineId::from(record.target.endpoint.service_id),
            record.push_id,
        ),
        machine_label: MachineLabel::try_from("Legacy".to_owned()).unwrap(),
        origin: record.origin,
        header_facts: record.header_facts,
        body: None,
    })
    .unwrap();
    assert_eq!(
        line,
        "🧵 Router: new thread activity @Legacy · 1 thread · 7 messages · router://00000000-0000-4000-8000-000000000001/push/01900000-0000-7000-8000-000000000200"
    );
}

#[test]
fn known_unsent_follow_receipts_can_remain_held_but_other_kinds_and_unknown_cannot() {
    use collaboration_protocol::{
        DeliveryNextAction, DeliveryOutcome, DeliveryReceipt, DeliveryRejection,
        DeliveryRejectionReason, MessageDelivery,
    };
    let follow = draft(vec![activity_range(1, 2, 300)], 1);
    let rejected = DeliveryReceipt {
        outcome: DeliveryOutcome::Rejected(DeliveryRejection {
            native_failure: None,
            reason: DeliveryRejectionReason::ProviderRejected,
            next_action: DeliveryNextAction::InspectTarget,
            client_code: Some(42),
            detail: Some("preserved known rejection".to_owned()),
            claims: None,
        }),
        reachability: None,
        client: None,
    };
    let not_submitted = DeliveryReceipt {
        outcome: DeliveryOutcome::NotSubmitted {
            retryable: false,
            reason: "known unsent".to_owned(),
        },
        reachability: None,
        client: None,
    };
    for receipt in [rejected, not_submitted] {
        let stored = PushRecord::from_storage_parts(
            follow.clone(),
            PushDeliveryState::Held,
            Some(receipt.clone()),
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(stored.last_outcome).unwrap(),
            serde_json::to_value(Some(receipt.clone())).unwrap()
        );
        let mut direct = follow.clone();
        direct.kind = PushKind::DirectMessage;
        direct.origin = PushOrigin::OwnerUnverified;
        direct.origin_router_ref = None;
        direct.mode = Some(MessageDelivery::Auto);
        direct.header_facts = PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        };
        direct.body = Some("direct message".to_owned());
        direct.activity = None;
        assert!(
            PushRecord::from_storage_parts(
                direct,
                PushDeliveryState::Held,
                Some(receipt),
                None,
                None,
                None,
            )
            .is_err()
        );
    }
    for outcome in [DeliveryOutcome::Unknown, DeliveryOutcome::Started] {
        assert!(
            PushRecord::from_storage_parts(
                follow.clone(),
                PushDeliveryState::Held,
                Some(DeliveryReceipt {
                    outcome,
                    reachability: None,
                    client: None
                }),
                None,
                None,
                None,
            )
            .is_err()
        );
    }
}

#[test]
fn overlapping_ranges_for_the_same_thread_are_refused_in_either_order() {
    for ranges in [
        vec![activity_range(1, 2, 300), activity_range(1, 100, 400)],
        vec![activity_range(1, 100, 400), activity_range(1, 2, 300)],
        vec![activity_range(1, 2, 300), activity_range(1, 300, 400)],
    ] {
        assert!(draft(ranges, 1).into_pending().is_err());
    }
}

#[test]
fn twenty_one_ranges_are_refused_even_when_only_one_thread_is_involved() {
    for one_thread in [false, true] {
        let ranges = (0_u64..21)
            .map(|index| {
                activity_range(
                    if one_thread { 1 } else { index + 1 },
                    index * 10 + 1,
                    index * 10 + 2,
                )
            })
            .collect();
        assert!(
            draft(ranges, if one_thread { 1 } else { 21 })
                .into_pending()
                .is_err()
        );
    }
}

#[test]
fn disjoint_ranges_for_one_thread_keep_both_bounds_and_require_one_header_thread() {
    let ranges = vec![activity_range(1, 2, 300), activity_range(1, 400, 450)];
    let record = draft(ranges.clone(), 1).into_pending().unwrap();
    assert_eq!(record.activity.unwrap().ranges, ranges);
    assert!(draft(ranges, 2).into_pending().is_err());
}
