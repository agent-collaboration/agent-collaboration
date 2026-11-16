import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/collaboration-service/src/schedule_preparation.rs",
    line: 135,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "serde_json::to_vec of (scheduleId, destination) stored as ExternalAdmission.canonical_request bytes, the idempotency key compared on retry (L135-153)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
      {
        crate: "automation-storage",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/schedule_preparation.rs",
    line: 174,
    owner: "router-vocabulary",
    kind: "content-parser",
    defines:
      "retained preparation failure: stored serde_json::Value upgraded by collaboration_runtime upgrade_stored_failure then decoded with from_value::<ScheduleFailure> (L174-179); an undecodable value becomes InvalidRecord",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "legacy-stored-decoders",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/schedule_preparation.rs",
    line: 342,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ScheduleFailure (MCP schedule_prepare failure data) built by hand at reject_choice_field L342, reject_preselection L371, error L398 and reject_without_route L477 with literal messages; kind to nextAction table L409-414; field/constraint derived from kind InvalidField (L378-379, L405-406); public_effects (L464) silently falls back to Local{mutation: None} when route evidence cannot be projected",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/schedule_projection.rs",
    line: 11,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "stored ScheduleDefinition (L11-14) and imported_continuity (L15-18) converted to the MCP ScheduleSnapshot types by serde_json to_value then from_value",
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
    file: "crates/collaboration-service/src/subscription_delivery/follow_window_proposals.rs",
    line: 226,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "follow_coverage: mail NotificationSource::FollowBatch membership (MembershipId) compared with the board membership RecordId by as_str (L226)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
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
    file: "crates/collaboration-service/src/subscription_delivery/reader_follow_allocations.rs",
    line: 100,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "allocate_due_follow: target SessionRef endpoint service_id (UuidIdentity) to message_board ServiceId by String round trip (L100-102) for follow recipient enrollment",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "service-identity-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/reader_poll_mail.rs",
    line: 193,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "hand_over_poll_mail rebuilds PushLineInput with RouterLink(MachineId::from(service_id), pushId) and body None for the MCP wait notice line (fourth copy of push_record_resolver.rs:22)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/reader_wait_handoff.rs",
    line: 78,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "hand_to_waiter records a hand-built DeliveryReceipt: Started when the MCP wait reply was accepted, else NotSubmitted{retryable:false, reason literal 'wait caller disconnected before handoff'} (L78-93)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_delivery_view.rs",
    line: 103,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "delivery_view: mail FollowBatch MembershipId compared with board RecordId by as_str (L103), the same compare as follow_window_proposals.rs:226",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_delivery_view.rs",
    line: 125,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "fleet_sync_protocol ItemId uuid to PushId by to_string().try_into() (L125-130) to look up the mail delivery observation; cf. MailItemId to PushId at collaboration-protocol mail_boundary_conversions.rs:66",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "fleet-item-identity",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_delivery_view.rs",
    line: 188,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "DeliveryOutcome to message_board SubscriptionDeliveryOutcome: NotSubmitted{reason, retryable} copied; Unknown to Unknown{evidence}; Rejected to Rejected{evidence}; every other outcome (Started, Queued, ...) to Accepted (L188-212); shown in MCP thread_subscriptions last_outcome",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_delivery_view.rs",
    line: 193,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "SubscriptionDeliveryOutcome evidence is the whole DeliveryReceipt as a serde_json::Value (L193, L209) with a hand-inserted routeEvidence key holding RouteEffectEvidence::ProviderAcp JSON (L194-204): typed receipt and route evidence carried as untyped JSON inside a typed MCP value",
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
    file: "crates/collaboration-service/src/subscription_delivery/subscription_window_error.rs",
    line: 104,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "hand BoardError bodies: invalid_window {InvalidRecord, Storage, literal message, InspectResource} (L104) and invalid_membership {InvalidRecord, Inspection, storage error text, FieldConstraint{field, requirement literal}} (L113); ProjectStorageError and ProjectRegistryError classified into definitive Project vs RetryProject failures (L50-93)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/subscription_delivery/subscription_window_selection.rs",
    line: 17,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "thread_project: BoardError::resource_not_found(Thread) with message overwritten by a literal (L17-21)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/wakeup_projection.rs",
    line: 17,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "hand copy of agent_automation FirstFire {wakeupId,occurrenceId,dueAtMs,firedAtMs} into protocol FireReceipt {kind:wakeFired,dueAt,firedAt} for MCP WakeSnapshot.firstFire; second copy wakeup_subscription.rs:163",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/wakeup_projection.rs",
    line: 26,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "enum-to-enum mapping agent_automation::WakeState -> collaboration_protocol::WakeState (Active, Paused, Cancelled, Expired, Finished): two identical enums for one meaning",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "wake-record-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/wakeup_projection.rs",
    line: 34,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      'WakeSnapshot.latestEventCursor: JSON tuple [1, serviceId, "automation-events", latestEventSequence, observedAtMs] hand-encoded (l34-41), an independent copy of the canonical automation_event_cursor.rs:4 encoder (also WakeSubscription.after, wakeup_subscription.rs:38)',
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "automation-event-cursor",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/wakeup_projection.rs",
    line: 48,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "agent_automation TimingRule -> protocol TimingRequest by serde_json to_value then from_value (WakeDefinition.timing in MCP WakeSnapshot)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/wakeup_projection.rs",
    line: 74,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "retained RetainedDelivery = DeliveryRecord<Value,Value,Value> -> DeliveryRecord<SessionRef, CodexGeneration, StoredDeliveryReceipt> by serde_json to_value then from_value, projected into WakeMutationResult.dispatchedDeliveries",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/wakeup_subscription.rs",
    line: 79,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      'wake wait cursor (MCP wake_wait_until_first_fire after/cursor) parsed with serde_json::from_str as (u8, UuidIdentity, String, i64, i64) and checked version 1, service, "automation-events"; rebuilt by to_string at l113 and l185, third copy of the builder at wakeup_projection.rs:34',
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "automation-event-cursor",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/wakeup_subscription.rs",
    line: 149,
    owner: "router-vocabulary",
    kind: "content-parser",
    defines:
      "stored wake transition kind strings -> protocol WakeChange: fired (body.fire decoded by from_value into FirstFire l151) -> Fired, paused -> Paused, cancelled|replaced -> Cancelled, expired|upgradeDuplicate|upgradeLimit -> Expired, finishedWithoutFiring -> FinishedWithoutFiring; created|resumed|finished|coalesced skipped; other -> error",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/automation_collection_cursor.rs",
    line: 8,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "CollectionCursor {version, serviceId, collection, upperKey, lastKey, filterDigest} encoded as base64url JSON (encode l20, decode l25 with version/service/collection/digest checks): the one opaque page cursor codec for automation collections (used by wake_operations.rs:189,275 and automation_history_operations.rs:157); boundary encoding, no defect",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "automation-collection-cursor",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/automation_event_cursor.rs",
    line: 4,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      'automation-events cursor: JSON tuple [1, serviceId, "automation-events", sequence, observedAtMs] (encode l4, decode l14 with version/service/collection checks): the canonical opaque event cursor codec, consumed by automation_history_operations.rs:209,248-250; boundary encoding, no defect (independent copies: automation_operations.rs:97, wakeup_projection.rs:34, wakeup_subscription.rs:79)',
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "automation-event-cursor",
    disposition: "boundary-encoding",
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
    file: "crates/collaboration-service/src/collaboration_application/board_paging.rs",
    line: 12,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "MetadataCursor {operation, filter, lastKey} (l12) and ProjectReadCursor {operation, filter, highWater, next} (l19) signed into opaque board page cursors by message-board-storage encode_board_cursor (l66-75, l134-143) and decoded l97, l168; fingerprint (l25-31) serializes the request minus page into the cursor's filter string; boundary encoding, no defect",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "board-page-cursor",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/board_participant_pages.rs",
    line: 16,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "ParticipantPageCursor {operation, filter, lastKey} with nested message-board-storage ParticipantIntervalKey (project_participant_pages.rs:10) as lastKey, signed by encode_board_cursor (l142-149) and decoded l52; opaque participant page cursor; boundary encoding, no defect",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "board-page-cursor",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/board_receipt_pages.rs",
    line: 13,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "ReceiptContinuation {scope, request, count, digest, cursor} signed by encode_board_cursor (l126-137) into the opaque request_effects continuation and decoded l26-30 with scope/request checks; boundary encoding, no defect",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "board-page-cursor",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/board_inbox_reads.rs",
    line: 13,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "InboxReadFilter {scope, reader, readMode} serialized only by board_paging::fingerprint into the inbox cursor filter string (l24-29); InboxScope mapped variant by variant onto PostListScope (l76-82) for the inbox post read",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "board-page-cursor",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/board_discovery_reads.rs",
    line: 141,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "discovery_hit_key spells the discovery ordering key '0:'/'1:'/'2:' + id for Workspace/Board/Project hits (l141-146), the last_key placed in the signed discovery_search page cursor; the same key mapping is independently spelled in message-board-storage fleet_discovery_pages.rs:18-25,38-53",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "fleet-discovery-order-key",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/project_admission/mod.rs",
    line: 118,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ProjectAdmission::new: collaboration_contracts MachineIdentity machine_label to message_board MachineLabel by validated string (l118), a separate mapping from the service-id conversion at l116",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/board_operations.rs",
    line: 311,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "subscription_wait_result: internal SubscriptionWaitResult Notice/Ranges (subscription_wait.rs:11-27) mapped onto MCP ThreadSubscriptionWaitOutcome Batch{SubscriptionWaitBatch::Notice|Ranges} or TimedOut (l311-338)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/board_subscription_operations.rs",
    line: 169,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MCP ThreadSubscriptionWaitFilter All/Roots/Project mapped onto internal SubscriptionWaitFilter (l169-177)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/board_project_operations.rs",
    line: 348,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "post_references: MCP PostReferences ReferenceTarget Post/Thread mapped onto board RecordRef by variant (l348-356) for thread_create/thread_post project requests",
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
    file: "crates/collaboration-service/src/attempt_history_projection.rs",
    line: 7,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "delivery: stored agent_automation AttemptOutcome (with its stored receipt, projected by StoredDeliveryReceipt::into_public l24) mapped onto public DeliveryEvidence Dispatching/KnownNotSubmitted/... with effects (l7-86)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/delivery_projection.rs",
    line: 21,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "stored DeliveryStatus mapped onto public DeliveryDisposition (Pending/Retryable/Dispatching to Pending, Accepted, Discarded, Failed, Uncertain; l21-28) and the stored attempt onto public DeliveryEvidence (l30-117)",
    usedBy: [
      {
        part: "N11",
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
