import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/collaboration-service/src/subscription_delivery/direct_message_push.rs",
    line: 49,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "hand DeliveryReceipt bodies: hold = NotSubmitted{retryable:true, reason literal 'target is not running; direct message held'} (L49); reject = Rejected{StaleGeneration when guarded else SteerUnsupported, nextAction InspectTarget, detail literal} (L66-88); stored on the push record shown by MCP message tools",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/direct_message_push.rs",
    line: 104,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "deliver_direct_message rebuilds PushLineInput with RouterLink(MachineId::from(service_id), pushId) and the record body for the delivered push line (second copy of push_record_resolver.rs:22)",
    usedBy: [
      {
        part: "N10",
      },
      {
        part: "N6",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/direct_message_push.rs",
    line: 124,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "PushId to DeliveryCorrelationId by String round trip (L124) for the direct-message DeliveryRequest",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "operation-identity-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/native_mail_attempt.rs",
    line: 54,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "Rejected{NoRoute} rewritten as a holdable NotSubmitted with reason literal (L54-64); AttemptReconciliation::KnownNotSubmitted to NotSubmitted 'queued input was not submitted' with ProviderAcp reachability (L93-100); StillUnknown/Err to unknown with provider evidence (L101-106)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/reader_delivery_owner.rs",
    line: 411,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "refuse_expired_pending_waiters: BoardError::board_unavailable with message overwritten by the literal 'Subscription scan did not succeed before the wait deadline. Retry later.' for the MCP wait reply (L411-415)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_push.rs",
    line: 133,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "expiry push: SubscriptionScope rendered as display text '{kind} {id}' in PushHeaderFacts::SubscriptionExpiry.scope (l133; escaped and displayed by collaboration-protocol push_line_render.rs:157-162) and a human-facing renew hint 'Renew with: agent-collaboration thread subscribe --thread-id|--project-id {id}' in the body (l135-137); the typed RouterOriginRef keeps the real SubscriptionScope (l127-132)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_push.rs",
    line: 159,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "RouterOriginRef (SubscriptionActivity{target, batchId} L98, SubscriptionExpiry{target, scope, subscriptionGeneration} L127) carried as its canonical JSON string in PushRecordDraft.origin_router_ref (canonical_string, collaboration-protocol router_origin_ref.rs:174)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
      {
        crate: "interaction-storage",
      },
    ],
    duplicateGroup: "router-origin-ref",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_push.rs",
    line: 182,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "store_prepared rebuilds PushLineInput with RouterLink(MachineId::from(service_id), pushId) and body None for subscription push lines (third copy of push_record_resolver.rs:22)",
    usedBy: [
      {
        part: "N10",
      },
      {
        part: "N6",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_push.rs",
    line: 220,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "PushId to DeliveryCorrelationId by String round trip (L220) for the subscription DeliveryRequest; same as direct_message_push.rs:124",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "operation-identity-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_push.rs",
    line: 295,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "target_session: board Identity::Session SessionRef to protocol SessionRef by serde_json to_value then from_value, by hand instead of the TryFrom at collaboration-protocol mail_boundary_conversions.rs:17",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_service.rs",
    line: 690,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "reader_for_target: protocol SessionRef to board SessionRef by serde_json to_value then from_value, wrapped as Identity::Session, by hand instead of the TryFrom at collaboration-protocol mail_boundary_conversions.rs:9",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 44,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "InventoryCursor {endpoint, view, generation, expiresAt, nativeCursor, storedTime, storedId, includeEmptySessions, scope, source, query}: private struct serialized to base64url JSON by encode (l315-321) and decoded from the request cursor (l225-233); the opaque continuation of native session_list pages; boundary encoding, no defect",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "native-inventory-cursor",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_push.rs",
    line: 84,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "SubscriptionBatch roots copied field by field into PushActivitySnapshot {ranges: PushActivityRange{threadId, projectId, from, through}, held, draining} for the subscription push RouterOriginRef (l84-97)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/push_record_resolver.rs",
    line: 63,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "make_notice_list: stored PushRecord copied field by field into public PushRecordNotice with its rendered line and RouterLink (l63-84)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
];
