import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/lifecycle-observation",
  files: [
    {
      file: "crates/lifecycle-observation/src/address_book_rebuild.rs",
      disposition: { kind: "none" },
      note: "journal rebuild transaction; decodes stored LifecycleObservation and AddressEntry JSON from SQL text columns (35,63-64,80) and keys thread_addresses by serde_json::to_string(ThreadAddress) (69-70); JSON-in-SQL storage covered by the journal_schema.sql:2 row, no message shape defined or copied",
    },
    {
      file: "crates/lifecycle-observation/src/address_snapshot_cache.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "pages cached AddressEntry rows into collaboration-protocol AddressPage (119-127); defines the opaque base64url snapshotId:offset cursor codec of AddressPage.next_cursor (encode 126, decode 64-82), indexed as a boundary-encoding row",
    },
    {
      file: "crates/lifecycle-observation/src/address_snapshot_reader.rs",
      disposition: { kind: "none" },
      note: "historical snapshot reader; decodes stored AddressEntry JSON (32-33) from SQL, covered by the journal_schema.sql:2 row",
    },
    {
      file: "crates/lifecycle-observation/src/inventory_reconciliation.rs",
      disposition: { kind: "none" },
      note: "per-observer read/notification ordering over SessionId thread keys; no serde, no message shape",
    },
    {
      file: "crates/lifecycle-observation/src/journal_migrations.rs",
      disposition: { kind: "none" },
      note: "SQL migration ownership; the last_seen_ms backfill decodes checkpoint AddressEntry JSON and checks its address_key equals serde_json::to_string(entry.address) (101-116), storage only",
    },
    {
      file: "crates/lifecycle-observation/src/journal_migrations/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared #[cfg(test)] mod tests at journal_migrations.rs:208-209",
      },
    },
    {
      file: "crates/lifecycle-observation/src/journal_page_reader.rs",
      disposition: { kind: "none" },
      note: "cursor scans returning collaboration-protocol JournalPage/LifecycleRecord; decodes stored LifecycleObservation JSON (59) and sizes records with serde_json::to_vec (71); storage only",
    },
    {
      file: "crates/lifecycle-observation/src/journal_retention.rs",
      disposition: { kind: "none" },
      note: "checkpoint and expiry; decode_checkpoint_address (24-40) and expiry rewrite (181-216) re-encode stored LifecycleObservation and AddressEntry JSON and ThreadAddress JSON keys; storage only",
    },
    {
      file: "crates/lifecycle-observation/src/journal_schema.sql",
      disposition: { kind: "none" },
      note: "SQL DDL; observation_json and entry_json TEXT columns are the storage codec of the canonical LifecycleObservation and AddressEntry serde types, indexed once as the journal_schema.sql:2 boundary-encoding row; storage, not a JSON string nested inside a wire message",
    },
    {
      file: "crates/lifecycle-observation/src/lib.rs",
      disposition: { kind: "none" },
      note: "crate root; re-exports collaboration-protocol records (AddressEntry, JournalPosition, AddressPage, CoverageView...), no shape of its own",
    },
    {
      file: "crates/lifecycle-observation/src/lifecycle_store.rs",
      disposition: { kind: "none" },
      note: "storage owner; the serde_json::from_value(json!) at 319 is inside the #[cfg(test)] mod tests declared at 261-262",
    },
    {
      file: "crates/lifecycle-observation/src/lifecycle_store/retention_worker.rs",
      disposition: { kind: "none" },
      note: "retention loop and its cfg(test) observation hooks (137-216); no message shape",
    },
    {
      file: "crates/lifecycle-observation/src/native_lifecycle_mapping.rs",
      disposition: { kind: "reads", owners: ["codex"] },
      note: "maps Codex notifications thread/started, thread/status/changed, thread/archived, thread/unarchived, thread/deleted, thread/closed and turn/completed (22-31) into LifecycleObservation by Value::get; thread.status and params.status decode by serde_json::from_value into collaboration-protocol NativeThreadStatus (38-44,54-60); turn.status hand-matched completed/interrupted/failed to TerminalStatus (69-74); Codex thread id and turn id strings converted to collaboration-protocol SessionId by parse_id (114-122) and wrapped in ThreadAddress {endpoint,native_thread_id} (99-104)",
    },
    {
      file: "crates/lifecycle-observation/src/native_observation_stream.rs",
      disposition: { kind: "sends", owners: ["codex"] },
      note: "request_validated thread/loaded/list {limit,cursor} (91-94) and thread/read {threadId,includeTurns:false} (146-149) to the managed app-server; hand json! params; LoadedPage Deserialize (299) decodes data straight into SessionId; thread/read reply hand-read by Value::get: thread.id compared to the requested id and thread.status decoded to NativeThreadStatus (155-167); SessionId to threadId by String::from (149,157); drains notifications and calls map_native_lifecycle (215-243)",
    },
    {
      file: "crates/lifecycle-observation/src/observation_coverage.rs",
      disposition: { kind: "none" },
      note: "disposable coverage state built from LifecycleChange into collaboration-protocol CoverageView (18-35); enum-to-state mapping inside the Router, no message shape",
    },
    {
      file: "crates/lifecycle-observation/src/observation_journal.rs",
      disposition: { kind: "none" },
      note: "journal append storage: LifecycleObservation and AddressEntry serialized to SQL text (87,129) with ThreadAddress JSON keys (110,179); storage only, covered by the journal_schema.sql:2 row",
    },
    {
      file: "crates/lifecycle-observation/src/thread_address_book.rs",
      disposition: { kind: "none" },
      note: "address reducer over collaboration-protocol AddressEntry and LifecycleChange (7-70); no serde, no message shape",
    },
  ],
  sendSites: [
    {
      file: "crates/lifecycle-observation/src/native_observation_stream.rs",
      line: 91,
      edge: "managed-codex",
      sends: "thread/loaded/list {limit,cursor}",
      owner: { crate: "lifecycle-observation" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "the N10 tap sends thread/read and turn/start probes; this caller and thread/loaded/list are in no tap",
      },
    },
    {
      file: "crates/lifecycle-observation/src/native_observation_stream.rs",
      line: 146,
      edge: "managed-codex",
      sends: "thread/read {threadId,includeTurns:false}",
      owner: { crate: "lifecycle-observation" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "the N10 tap records thread/read with a probe thread id, not this lifecycle observer call",
      },
    },
  ],
};
