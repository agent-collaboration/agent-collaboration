import type { LedgerSection } from "../schema-audit-types.ts";

const NOT_A_PROTOCOL =
  "Router-private SQLite account/quota store, not one of the four protocols or six outside edges";
const MIGRATIONS_TESTS_CHAIN =
  "cfg(test) module declared at account_migrations.rs:463-464 (#[cfg(test)] mod tests), which declares this file at account_migrations/tests.rs";
const STATE_TESTS_CHAIN =
  'cfg(test) module declared at lib.rs:26-28 (#[cfg(test)] #[path = "state_tests.rs"] mod tests), which declares this file at state_tests.rs';

export const section: LedgerSection = {
  root: "crates/codex-router-state",
  files: [
    {
      file: "crates/codex-router-state/src/account.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: AccountStatus (account.rs:8) SQLite text enabled/disabled and AccountRecord (account.rs:38) row value; no serde, no message shape`,
    },
    {
      file: "crates/codex-router-state/src/account_migrations.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: SQLx MIGRATOR (account_migrations.rs:15), legacy-to-baseline conversion (:232-300) and CriticalStateSnapshot table capture (:343-431); SQL only`,
    },
    {
      file: "crates/codex-router-state/src/account_migrations/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at account_migrations.rs:463-464 (#[cfg(test)] mod tests)",
      },
      note: "migration test root; declares fixtures (tests.rs:10) and six test modules (tests.rs:14-20)",
    },
    {
      file: "crates/codex-router-state/src/account_migrations/tests/fixtures.rs",
      disposition: { kind: "excluded", reason: `${MIGRATIONS_TESTS_CHAIN}:10` },
      note: "SQLite migration fixtures and child-process helpers",
    },
    {
      file: "crates/codex-router-state/src/account_migrations/tests/history_contracts.rs",
      disposition: { kind: "excluded", reason: `${MIGRATIONS_TESTS_CHAIN}:14` },
      note: "SQLx migration history tests",
    },
    {
      file: "crates/codex-router-state/src/account_migrations/tests/legacy_shapes.rs",
      disposition: { kind: "excluded", reason: `${MIGRATIONS_TESTS_CHAIN}:15` },
      note: "legacy SQLite shape migration tests",
    },
    {
      file: "crates/codex-router-state/src/account_migrations/tests/presence_fixtures.rs",
      disposition: { kind: "excluded", reason: `${MIGRATIONS_TESTS_CHAIN}:16` },
      note: "marker-only SQLite database fixtures",
    },
    {
      file: "crates/codex-router-state/src/account_migrations/tests/provider_migration.rs",
      disposition: { kind: "excluded", reason: `${MIGRATIONS_TESTS_CHAIN}:17` },
      note: "accounts.provider column migration tests",
    },
    {
      file: "crates/codex-router-state/src/account_migrations/tests/schema_contracts.rs",
      disposition: { kind: "excluded", reason: `${MIGRATIONS_TESTS_CHAIN}:18` },
      note: "fresh-schema contract tests",
    },
    {
      file: "crates/codex-router-state/src/account_migrations/tests/schema_integrity.rs",
      disposition: { kind: "excluded", reason: `${MIGRATIONS_TESTS_CHAIN}:19` },
      note: "schema trigger/constraint rejection tests",
    },
    {
      file: "crates/codex-router-state/src/account_migrations/tests/transaction_recovery.rs",
      disposition: { kind: "excluded", reason: `${MIGRATIONS_TESTS_CHAIN}:20` },
      note: "migration rollback tests",
    },
    {
      file: "crates/codex-router-state/src/account_routing_policy.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: WeeklyQuotaFloorBasisPoints (account_routing_policy.rs:15) and AccountRoutingPolicy (:42) persisted policy values`,
    },
    {
      file: "crates/codex-router-state/src/account_schema.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: SQLite legacy/target schema validation (account_schema.rs:35-487), LeaseShape/LegacyShape (:22-33)`,
    },
    {
      file: "crates/codex-router-state/src/account_schema/definitions.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: ColumnSpec (definitions.rs:4) table column specs incl. SESSION_ACCOUNT_AFFINITIES (:207) and CURRENT_SESSION_ACCOUNT_AFFINITIES (:212); SQLite DDL expectations only`,
    },
    {
      file: "crates/codex-router-state/src/account_schema/validation.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: PRAGMA table/index validation and schema_tokens SQL tokenizer (validation.rs:399); parses SQLite DDL, not protocol JSON`,
    },
    {
      file: "crates/codex-router-state/src/affinity_owner.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: PreviousResponseAffinityOwnerRecord (affinity_owner.rs:39) keyed by a hashed OpenAI Responses previous_response_id (model-traffic HTTP/SSE or WebSocket, AffinitySourceTransport :9), not Codex app-server/ACP/MCP or a Router SessionRef`,
    },
    {
      file: "crates/codex-router-state/src/credential_maintenance.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: CredentialMaintenanceState (credential_maintenance.rs:8), CredentialFailureClass (:41), ClaimPurpose (:101) SQLite text enums for OAuth renewal state`,
    },
    {
      file: "crates/codex-router-state/src/credential_maintenance_store.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: SQLite credential renewal claim transitions (credential_maintenance_store.rs:19-614) and row decoding decode_maintenance_row (:649)`,
    },
    {
      file: "crates/codex-router-state/src/credit_store.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: CreditRefreshAttempt (credit_store.rs:13), ResponsesRefreshSuccessCommit (:40), CreditUsageObservation (:59) wrap OpenAI quota/credit facts (outside the four protocols)`,
    },
    {
      file: "crates/codex-router-state/src/credit_store/observation_store.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: StoredCreditUsageObservationRow (observation_store.rs:20) decoded into CreditProviderObservation (:32-117); OpenAI credit facts at rest`,
    },
    {
      file: "crates/codex-router-state/src/credit_store/policy_mutation_store.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: AsyncCreditUsagePolicyMutationStore (policy_mutation_store.rs:16) generation-checked allow_credits write`,
    },
    {
      file: "crates/codex-router-state/src/credit_store/policy_store.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: account_credit_policies allow_credits 0/1 read (policy_store.rs:13) and fixture save (:36)`,
    },
    {
      file: "crates/codex-router-state/src/credit_store/refresh_transaction.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: ordered OpenAI quota refresh attempt allocation (refresh_transaction.rs:21) and atomic commits (:158, :345)`,
    },
    {
      file: "crates/codex-router-state/src/credit_store/tests.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test) module declared at credit_store.rs:161-162 (#[cfg(test)] mod tests)",
      },
      note: "credit store unit tests",
    },
    {
      file: "crates/codex-router-state/src/future_send_contract.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test) module declared at lib.rs:30-31 (#[cfg(test)] mod future_send_contract)",
      },
      note: "compile-time Send contract test for AsyncSqliteStateStore futures",
    },
    {
      file: "crates/codex-router-state/src/lib.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: module declarations and package_name (lib.rs:1-31)`,
    },
    {
      file: "crates/codex-router-state/src/provider_proxy_store.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: provider-proxy.sqlite path helper (provider_proxy_store.rs:4-10)`,
    },
    {
      file: "crates/codex-router-state/src/quota_snapshot.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: QuotaSnapshotSource (quota_snapshot.rs:49), QuotaRefreshErrorClass (:113), SelectorQuotaWindowStatus (:100), PersistedQuotaSnapshot (:87), SelectorQuotaInput (:568) quota selector rows; no serde`,
    },
    {
      file: "crates/codex-router-state/src/repositories.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: sync repository traits AccountStateRepository (repositories.rs:18), QuotaSnapshotRepository (:33), SelectorQuotaRepository (:52), AffinityRepository (:93)`,
    },
    {
      file: "crates/codex-router-state/src/selection_projection.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: projects stored quota rows into codex-router-selection inputs (selection_projection.rs:241-508); SelectorQuotaWindowStatus->QuotaWindowStatus map (:848) is internal selection vocabulary, not session/approval/identity vocabulary`,
    },
    {
      file: "crates/codex-router-state/src/session_account_affinity.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: SessionAccountAffinity (session_account_affinity.rs:38) keys an opaque provider model-traffic session_id String with Provider; caller is codex-router-proxy session_account_affinity_cache.rs (Claude edge pins); no SessionRef, thread id or Codex shape`,
    },
    {
      file: "crates/codex-router-state/src/sqlite.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: StateStoreError (sqlite.rs:124), SqliteStateStore/AsyncSqliteStateStore (:252, :269) and integer conversion helpers (:482-526)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/account_removal.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: transactional disabled-account deletion across owned tables (account_removal.rs:47-120)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/account_store.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: accounts upsert/list/load and credential-mutation quota invalidation (account_store.rs:18-407)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/active_leases.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: active_client_leases and active_session_events rows keyed by ReservationId (active_leases.rs:6-163); logical_session_id is the reservation id, not a Router SessionRef`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/affinity_store.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: session_account_affinities (provider, session_id) CAS pin writes (affinity_store.rs:65-198, row parse :285) and hashed previous-response owners (:12, :314); opaque strings, no SessionRef conversion`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/legacy_schema_v1to5.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: sync-rusqlite-fixtures/test-gated (sqlite.rs:7-8) legacy v1-v5 DDL (legacy_schema_v1to5.rs:7-261)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/legacy_schema_v6to10.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: sync-rusqlite-fixtures/test-gated (sqlite.rs:9-10) legacy v6-v10 DDL statements (legacy_schema_v6to10.rs:4-340)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/policy_mutation.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: weekly-floor and account-status mutations with busy retry (policy_mutation.rs:37-366)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/quota_history.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: quota_history_observations append/query/prune and row parse (quota_history.rs:10-244)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/quota_refresh_status.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: quota_refresh_status success metadata upsert (quota_refresh_status.rs:9-104)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/quota_snapshots.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: quota_snapshots upsert/load and selector_window_from_snapshot (quota_snapshots.rs:17-220)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/repository_contracts.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: async repository traits incl. AsyncSessionAccountAffinityRepository (repository_contracts.rs:120) over opaque provider session_id`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/selector_windows.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: selector_quota_windows / route_band_account_states reads and writes (selector_windows.rs:43-749)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/session_history.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: active_session_events queries and compaction (session_history.rs:6-124); model-traffic lease intervals, not Router session events`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/session_records.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: ActiveSessionEventKind (session_records.rs:13) SQLite text acquired/released/retired/stale_purged, ActiveSessionEvent (:49), ActiveSessionRollup (:135), ActiveClientCount (:5); lease accounting, not the Router SessionEvent vocabulary`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/session_rollups.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: active_session_rollups interval bucketing (session_rollups.rs:7-395)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/sync_accounts.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: sync-rusqlite-fixtures/test-gated (sqlite.rs:21-22) rusqlite account store (sync_accounts.rs:11-356)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/sync_affinity.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: sync-rusqlite-fixtures/test-gated (sqlite.rs:23-24) affinity_pins and previous-response owner repository (sync_affinity.rs:10-141)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/sync_migrations.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: sync-rusqlite-fixtures/test-gated (sqlite.rs:25-26) rusqlite migration ladder and v11-v13 DDL (sync_migrations.rs:23-306)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/sync_selectors.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: sync-rusqlite-fixtures/test-gated (sqlite.rs:27-28) selector window repository (sync_selectors.rs:14-559)`,
    },
    {
      file: "crates/codex-router-state/src/sqlite/sync_snapshots.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: sync-rusqlite-fixtures/test-gated (sqlite.rs:29-30) quota snapshot repository (sync_snapshots.rs:9-234)`,
    },
    {
      file: "crates/codex-router-state/src/state_tests/policy_schema_fixtures.rs",
      disposition: { kind: "excluded", reason: `${STATE_TESTS_CHAIN}:58-59` },
      note: "policy schema version fixtures",
    },
    {
      file: "crates/codex-router-state/src/state_tests/quota_schema_fixtures.rs",
      disposition: { kind: "excluded", reason: `${STATE_TESTS_CHAIN}:61-62` },
      note: "quota schema version fixtures",
    },
    {
      file: "crates/codex-router-state/src/state_tests/session_schema_fixtures.rs",
      disposition: { kind: "excluded", reason: `${STATE_TESTS_CHAIN}:64-65` },
      note: "active-session schema version fixtures",
    },
    {
      file: "crates/codex-router-state/src/state_tests/test_identity_fixtures.rs",
      disposition: { kind: "excluded", reason: `${STATE_TESTS_CHAIN}:55-56` },
      note: "temp dir, account id and affinity hash test fixtures",
    },
    {
      file: "crates/codex-router-state/src/window_observation.rs",
      disposition: { kind: "none" },
      note: `${NOT_A_PROTOCOL}: WindowObservation (window_observation.rs:85) and WindowRejection (:212) Claude quota window rows at rest (Anthropic quota API facts, outside the four protocols)`,
    },
  ],
  sendSites: [],
};
