import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 22,
    owner: "codex",
    kind: "rust-type",
    defines:
      "ThreadTurnsPage, ThreadTurn, ThreadTurnItem (tag type, UserMessage or other): private Deserialize copy of the thread/turns/list reply, decoded at l623",
    usedBy: [
      {
        part: "N10",
      },
    ],
    duplicateGroup: "codex-hand-shapes",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 61,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "classify_source: Codex source/thread_source strings (subagent, guardian_review, memory_consolidation, any source containing 'subagent') to NativeSessionSource Subagents, else Interactive; one local mapping used for stored rows (l399) and loaded threads (l525); the direct read of source/threadSource from the thread/read reply is the l481 row",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "session-source-mapping",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 332,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "NativeSessionScope to StoredThreadRoot l332 and NativeSessionSource to StoredThreadSource l349; the Repo scope fields are copied again into RepositoryIdentity l87",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 384,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "Codex thread id string to collaboration_protocol::SessionRef {endpoint, session_id} through the existing typed SessionId TryFrom (stored l384, loaded l515) for the display-name cache; the session record target rebuilt by hand as {endpoint, sessionId} JSON is the l440 row",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 440,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "Router session record JSON {target,name,title,source,gitBranch,workingDirectory,observation,model,reasoningEffort,idleSeconds} built by json! (stored l440, loaded l540), a hand copy of the published NativeSessionListResult entry (collaboration-protocol native_session_catalog.rs:67-92)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "hand-session-records",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 477,
    owner: "codex",
    kind: "hand-json",
    defines:
      "hand-built Codex params: thread/loaded/list {limit,cursor} l477, thread/read l493, thread/turns/list l615",
    usedBy: [
      {
        part: "N10",
      },
    ],
    duplicateGroup: "codex-method-tables",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 481,
    owner: "codex",
    kind: "content-parser",
    defines:
      "thread/loaded/list reply data l481 and nextCursor l580; thread/read reply thread.id l498, status and status.type l504 l164, name, title, source, threadSource, gitInfo.branch and gitInfo.originUrl, cwd, model, reasoningEffort, updatedAt l151, preview l551, and status.turnId or /turn/id l168 (not in ActiveThreadStatus, per its own doc l160)",
    usedBy: [
      {
        part: "N10",
      },
    ],
    duplicateGroup: "codex-hand-shapes",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 633,
    owner: "codex",
    kind: "content-parser",
    defines:
      "is_unmaterialized_thread_rejection matches the Codex error text 'thread <id> is not materialized yet; thread/turns/list is unavailable before first user message' and code -32600",
    usedBy: [
      {
        part: "N10",
      },
    ],
    duplicateGroup: "codex-hand-shapes",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 652,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "page builds NativeSessionListResult as json! {endpoint, observedAt, generation, sessions, nextCursor} l652-657, then decodes it with serde_json::from_value l659: the outer page field names are a second hand copy of the published result (collaboration-protocol native_session_catalog.rs), not merely a JSON round trip",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "hand-session-records",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/interaction_operations.rs",
    line: 158,
    owner: "mcp",
    kind: "hand-json",
    defines:
      "inspect failure payload built by payload.insert: kind from ApprovalDecisionError::code() l155 for approval rejections, the error's detail() object merged in l163-165, fieldId l168 only on invalidAnswer, plus stage and message",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/interaction_operations.rs",
    line: 201,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "question_rejection: InteractionHistoryError to QuestionRejection (seven errors collapse into Unavailable l209-215); QuestionResponse to QuestionState l86; question_record_view projects stored QuestionHistoryState (answer/cancel payloads) onto the public unit QuestionState l241-246 (a different contract from the stored history, not a duplicate)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "failure-kind-mapping",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/interaction_operations.rs",
    line: 229,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "question_record_view: requester re-typed by serde_json to_value then from_value (l229-231); source (interaction-storage interaction_history.rs) and destination (collaboration-protocol question_contract.rs) are both message_board::SessionRef, so this is a same-type retyping, not a board-to-protocol SessionRef conversion",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/interaction_operations.rs",
    line: 236,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "session_event_model QuestionField converted to collaboration_protocol QuestionFieldView by serde_json to_value then from_value (two types for one question field)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "question-field-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/journal_operations.rs",
    line: 60,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "JournalError to JournalFailure: read_wait InvalidRecord, HistoryExpired and JournalChanged to CursorInvalidated, rest Unavailable (l60-76); address_snapshot SnapshotExpired, InvalidRecord, Capacity to Overloaded, rest Unavailable (l110-117)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "failure-kind-mapping",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/journal_operations.rs",
    line: 191,
    owner: "mcp",
    kind: "hand-json",
    defines:
      "JournalFailure bodies: invalidRequest {kind,message} l191, unavailable {kind,stage:discovery,message} l193, {kind:snapshotExpired} l195, cursor invalidation {kind:historyExpired|journalChanged,current} l196",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/message_operations.rs",
    line: 93,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "message_send: MailItemId converted to PushId by try_into (one message identity, two types)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/message_operations.rs",
    line: 384,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "delivery_failure: MailRefusal to MessageFailureKind (seven refusals collapse into Unavailable l405-414) and PushDeliveryFailure to kind and stage",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "failure-kind-mapping",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/message_operations.rs",
    line: 463,
    owner: "mcp",
    kind: "rust-type",
    defines:
      "MessageFailure {kind, stage, message, existing} with MessageFailureKind l473 and MessageFailureStage l497: message tool failure body defined in the service, not in collaboration-protocol",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/message_status_operations.rs",
    line: 105,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "message_show: PushId to MailItemId by try_into l105, then to fleet ItemId by Uuid::parse_str l109",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "fleet-item-identity",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/message_status_operations.rs",
    line: 246,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "authorize_mail_read: collaboration_protocol::SessionRef caller to message_board::SessionRef by try_into for the board Identity",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/message_status_operations.rs",
    line: 342,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "public_status: fleet_sync_protocol DeliveryStatus, DeliveryRefusal, DeliveryFailureKind, DeliverySizeField projected onto collaboration_protocol RemoteDeliveryStatus, RemoteDeliveryRefusal, RemoteProcessingFailure, RemoteSizeField (renames categories, e.g. AtRecipient to AtMachine l348)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/provider_observation_operations.rs",
    line: 253,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "attach: collaboration_protocol::SessionRef converted to message_board::SessionRef (SessionEventHub::attach takes the board type, collaboration-contracts session_event_hub.rs:56) by serde_json to_value then from_value",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/provider_observation_operations.rs",
    line: 269,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "event_value {sequence,event}: SessionEvent carried as a JSON value in an MCP result; resync marker l92, l135; EventTooLarge l285",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "session-event-envelope",
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/provider_observation_operations.rs",
    line: 320,
    owner: "mcp",
    kind: "hand-json",
    defines: "ObservationFailure {kind,stage,message}",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/session_native_control.rs",
    line: 51,
    owner: "codex",
    kind: "hand-json",
    defines:
      "hand-built Codex params: thread/read {threadId,includeTurns} l51, turn/interrupt {threadId,turnId} l112, thread/read l172, thread/name/set {threadId,name} l196",
    usedBy: [
      {
        part: "N10",
      },
    ],
    duplicateGroup: "codex-method-tables",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/session_native_control.rs",
    line: 74,
    owner: "codex",
    kind: "content-parser",
    defines:
      "Codex replies decoded by hand from serde_json::Value: thread/read result.thread and thread.id l74-80, /thread/name l188 and l223, turn/interrupt result compared with json!({}) l125, native error message l416",
    usedBy: [
      {
        part: "N10",
      },
    ],
    duplicateGroup: "codex-hand-shapes",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/collaboration-service/src/collaboration_application/session_native_control.rs",
    line: 402,
    owner: "router-vocabulary",
    kind: "method-table",
    defines:
      "classify_native_call_failure: NativeConnectionError to NativeSessionFailure (InvalidInput to InvalidRequest, Rejected to NativeRejected via collaboration_runtime::classify_native_rejection l415, Unavailable/UnavailableWithCause, otherwise OutcomeUnknown after a mutation or Unavailable)",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "failure-kind-mapping",
    disposition: "boundary-encoding",
  },
];
