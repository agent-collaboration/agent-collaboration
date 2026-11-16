import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/lifecycle-observation/src/journal_schema.sql",
    line: 2,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "storage codec, not a JSON string nested inside a wire message: observation_json and entry_json TEXT columns hold the canonical LifecycleObservation and AddressEntry serde encodings (observation_journal.rs:85-130), and address_key holds serde_json::to_string(ThreadAddress) (observation_journal.rs:110, address_book_rebuild.rs:69, journal_retention.rs:33); no independent copy of those types",
    usedBy: [{ crate: "lifecycle-observation" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/lifecycle-observation/src/native_lifecycle_mapping.rs",
    line: 21,
    owner: "codex",
    kind: "method-table",
    defines:
      "seven Codex notification names (thread/started, thread/status/changed, thread/archived, thread/unarchived, thread/deleted, thread/closed, turn/completed) matched as string literals (22-31,35-89); params, thread, threadId, turn and turn.id read by Value::get; thread status decoded into collaboration-protocol NativeThreadStatus by serde_json::from_value (38,54); turn.status hand-matched completed/interrupted/failed (69-74)",
    usedBy: [{ crate: "lifecycle-observation" }],
    duplicateGroup: "codex-method-tables",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/lifecycle-observation/src/native_lifecycle_mapping.rs",
    line: 114,
    owner: "codex",
    kind: "session-ref-conversion",
    defines:
      "Codex thread id (thread.id or params.threadId, l91) converted by parse_id into collaboration-protocol SessionId and wrapped in ThreadAddress {endpoint,native_thread_id} (99-104); native_observation_stream.rs converts back with String::from(SessionId) for thread/read (149,157) and decodes thread/loaded/list data directly as Vec<SessionId> (302). parse_id also types the Codex turn.id (l68) as a SessionId for LifecycleChange::TurnTerminal, though a turn id is a different identity from the thread/session id",
    usedBy: [{ crate: "lifecycle-observation" }],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/lifecycle-observation/src/native_observation_stream.rs",
    line: 91,
    owner: "codex",
    kind: "hand-json",
    defines:
      "hand-built params for thread/loaded/list {limit,cursor} (91) and thread/read {threadId,includeTurns:false} (146), validated by NativeOperation; thread/read reply hand-read by Value::get for thread.id and thread.status (155-167)",
    usedBy: [{ crate: "lifecycle-observation" }],
    duplicateGroup: "codex-method-tables",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/lifecycle-observation/src/native_observation_stream.rs",
    line: 299,
    owner: "codex",
    kind: "rust-type",
    defines: "stand-in LoadedPage {data,nextCursor} for the thread/loaded/list reply",
    usedBy: [{ crate: "lifecycle-observation" }],
    duplicateGroup: "codex-stand-in-schemas",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: "crates/lifecycle-observation/src/address_snapshot_cache.rs",
    line: 70,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "opaque address-snapshot cursor codec: AddressPage.next_cursor (MCP addresses_list) is base64url of the text <snapshotId>:<offset> (encoded in make_page 126), decoded and validated in page (64-82: length bound, base64url, UTF-8, split on ':', UuidIdentity, usize offset); the inner text is colon-separated, not JSON; the only codec of this cursor",
    usedBy: [
      { crate: "lifecycle-observation" },
      { crate: "collaboration-service" },
      { part: "N11" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
];
