// The old registry's library lease tables, checked the way main's first open
// checks them: fleet-sync-lease `validate_legacy_rows` (legacy_registry_rows.rs
// at the pinned library 0b27429), with the record types it decodes
// (registry_records.rs, workflow_values.rs, fleet-sync-protocol). Every row is
// decoded and every invariant main enforces is enforced here, before any write,
// so a value main would refuse stops the migration with its row named (R7).

import { parseSerdeJson } from "./exact_json.ts";
import {
  Invalid,
  adjacentlyTagged,
  bool,
  digest,
  epoch,
  fields,
  fromJsonText,
  internallyTagged,
  invalid,
  newtype,
  nonEmptyText,
  nonZeroU64,
  option,
  string,
  struct,
  syncTime,
  text,
  u64,
  unit,
  unitEnum,
  uuidV7,
  vec,
  type Decoder,
  type Tagged,
} from "./serde_shapes.ts";
import type { ReadReply, ReadRow } from "./storage_objects.ts";

// fleet-sync-protocol values.

const position = struct({ epoch, sequence: u64 }, false);
const schemaIdentity = struct({ version: u64, fingerprint: nonEmptyText }, true);
const schemaSupport = struct({ maxVersion: u64 }, true);
const gateReason = struct({ code: nonEmptyText, detail: string }, true);
const actorRef = text(1, 256);
const failureCode = nonEmptyText;
const origin = adjacentlyTagged("kind", "value", {
  empty: unit,
  seeded: fields({ position }),
});
const leaseStatus = struct(
  {
    database: uuidV7,
    epoch,
    holder: uuidV7,
    state: unitEnum(["held", "expired"]),
    expiresAt: syncTime,
    remainingMs: u64,
  },
  true,
);
const takeoverPreview = struct(
  { token: uuidV7, fence: epoch, acked: position, holder: uuidV7, lastSeen: option(syncTime) },
  true,
);
const holderChangeRequest = adjacentlyTagged("kind", "value", {
  move: fields({
    attempt: uuidV7,
    database: uuidV7,
    fromEpoch: epoch,
    to: uuidV7,
    reason: text(1, 16384),
  }),
  takeover: fields({
    attempt: uuidV7,
    database: uuidV7,
    preview: takeoverPreview,
    to: uuidV7,
    reason: text(1, 16384),
  }),
  claim: fields({ attempt: uuidV7, database: uuidV7, claimant: uuidV7 }),
});
const holderChangeRefusal = adjacentlyTagged("kind", "value", {
  holderStale: fields({ lastRenewedAt: syncTime }),
  gateRefused: fields({ reason: gateReason }),
  transferInProgress: fields({ transfer: uuidV7 }),
  previewStale: unit,
  migrationPending: fields({ migration: u64, state: unitEnum(["pending", "failed"]) }),
  schemaTooNew: fields({ store: schemaIdentity, supported: option(u64) }),
  alreadyHeld: fields({ holder: uuidV7 }),
  notFound: unit,
  unavailable: unit,
  epochUnprovable: unit,
  superseded: fields({ by: uuidV7 }),
  holderUnreachable: fields({ lastSeen: option(syncTime) }),
  retired: unit,
});
const holderChangeAnswer = adjacentlyTagged("kind", "value", {
  cancelled: fields({ by: actorRef }),
  granted: fields({ epoch, expiresAt: syncTime, start: position }),
  pending: fields({ transfer: uuidV7 }),
  refused: newtype(holderChangeRefusal),
  failed: fields({ reason: failureCode }),
});
const renewAnswer = adjacentlyTagged("kind", "value", {
  renewed: fields({
    recorded: struct({ epoch, expiresAt: syncTime }, true),
    current: leaseStatus,
    transferInProgress: option(uuidV7),
    replayed: bool,
  }),
  refused: newtype(
    adjacentlyTagged("kind", "value", {
      holderNotEnrolled: unit,
      staleEpoch: fields({ currentEpoch: epoch, epochEndedAt: position }),
      notWriter: fields({ current: option(epoch) }),
      noAuthoritativeCopy: unit,
      epochUnprovable: unit,
      notRegistered: unit,
      retired: unit,
    }),
  ),
});
const registration = struct({ attempt: uuidV7, database: uuidV7, writer: uuidV7, origin }, true);
const importRequest = struct({ attempt: uuidV7, database: uuidV7, digest }, true);
const retirement = struct(
  { attempt: uuidV7, database: uuidV7, expectFence: epoch, expectAcked: position },
  true,
);
const lifecycleIntent = adjacentlyTagged("kind", "request", {
  register: newtype(registration),
  import: newtype(importRequest),
  retire: newtype(retirement),
});
const lifecycleOutcome = adjacentlyTagged("kind", "answer", {
  registration: newtype(
    adjacentlyTagged("kind", "value", {
      registered: fields({ lease: leaseStatus, replayed: bool }),
      refused: newtype(
        adjacentlyTagged("kind", "value", {
          databaseRejected: fields({ reason: gateReason }),
          notWriter: fields({ current: option(epoch) }),
          retired: unit,
        }),
      ),
    }),
  ),
  import: newtype(
    adjacentlyTagged("kind", "value", {
      imported: unit,
      alreadyImported: unit,
      importConflict: fields({ existing: option(digest), offered: digest }),
      retired: unit,
    }),
  ),
  retirement: newtype(
    adjacentlyTagged("kind", "value", {
      retired: fields({ at: syncTime }),
      refused: newtype(
        adjacentlyTagged("kind", "value", {
          copyNotInitialized: unit,
          leaseHeld: unit,
          transferInProgress: unit,
          gateRefused: fields({ reason: gateReason }),
          copyMismatch: fields({ fence: epoch, acked: position }),
        }),
      ),
    }),
  ),
  failed: fields({ reason: failureCode }),
});

// fleet-sync-lease records (registry_records.rs, workflow_values.rs).

const databaseLifecycle = internallyTagged("state", {
  registered: { origin, importedFrom: option(struct({ digest }, true)) },
  unheld: { importDigest: digest },
  retiring: { attempt: uuidV7 },
  retired: { at: syncTime },
});
const databaseRecord = struct(
  {
    database: uuidV7,
    lifecycle: databaseLifecycle,
    revision: struct({ epoch, change: nonZeroU64 }, true),
    changed_at: syncTime,
  },
  true,
);
const registryLease = struct(
  { database: uuidV7, holder: uuidV7, epoch, expires_at: syncTime, last_renewed_at: syncTime },
  true,
);
const epochHistory = struct(
  { database: uuidV7, epoch, holder: uuidV7, granted_at: syncTime, ended_at: option(position) },
  true,
);
const renewalAttempt = struct(
  {
    request: struct({ attempt: uuidV7, database: uuidV7, epoch }, true),
    holder: uuidV7,
    answer: renewAnswer,
  },
  true,
);
const previewRecord = struct({ database: uuidV7, preview: takeoverPreview }, true);
const holderChangePhase = internallyTagged("phase", {
  requested: {},
  releasing: {},
  released: { finalPosition: position },
  reserved: {},
  fencing: {},
  cancelling: {},
  ended: {},
});
const phaseEntry = struct({ phase: holderChangePhase, enteredAt: syncTime }, true);
const cleanupFailure = struct({ reason: failureCode, at: syncTime }, true);
const cancelRecord = struct({ by: actorRef, reason: text(1, 512), at: syncTime }, true);
const reportSchemaSupport = struct(
  { peer: uuidV7, sessionId: uuidV7, attachSeq: nonZeroU64, support: schemaSupport },
  true,
);

const json = <T>(decoder: Decoder<T>): Decoder<T> => fromJsonText(parseSerdeJson, decoder);
/** An optional column: main reads a value not stored as text as absent (`CASE WHEN typeof(c)='text'`). */
const optionalJson =
  <T>(decoder: Decoder<T>) =>
  (value: unknown, path: string): T | null =>
    typeof value === "string" ? json(decoder)(value, path) : null;
/** `stored_scalar`: the column's text, decoded as a JSON string (registry_row.rs:23–30). */
const scalar =
  <T>(decoder: Decoder<T>): Decoder<T> =>
  (value, path) =>
    typeof value === "string"
      ? decoder(value, path)
      : invalid(path, "missing or not stored as text");
/** `stored_epoch`: canonical decimal text of an epoch (registry_row.rs:31–41). */
const storedEpoch: Decoder<bigint> = (value, path) =>
  typeof value === "string" && /^(0|[1-9]\d*)$/.test(value)
    ? epoch(BigInt(value), path)
    : invalid(path, "missing, not stored as text or not a canonical decimal epoch");

type Payload = Record<string, unknown>;
const payload = (tagged: Tagged): Payload => (tagged.value ?? {}) as Payload;

// Holder-change phase rules (registry_records.rs TransferRecord::allows/validate).

interface Transfer {
  readonly request: Tagged;
  readonly outcome: Tagged | null;
  readonly phase: Tagged;
  readonly history: readonly { readonly phase: Tagged; readonly enteredAt: bigint }[];
  readonly cleanupFailed: { readonly at: bigint } | null;
}
const CANCELLABLE = ["requested", "releasing", "released", "reserved", "cancelling"];
function allows(request: string, before: string, after: string): boolean {
  if (before === "ended") return false;
  if (after === "ended") return true;
  if (
    (before === "requested" && after === "releasing") ||
    (before === "releasing" && after === "released")
  )
    return request === "move";
  if (before === "requested" && after === "reserved") return request === "claim";
  if (before === "requested" && after === "fencing") return request === "takeover";
  if ((before === "released" || before === "reserved") && after === "fencing") return true;
  if (after === "cancelling") return CANCELLABLE.includes(before) && request === "move";
  return false;
}
function validTransfer(transfer: Transfer): boolean {
  const request = transfer.request.variant;
  const phase = transfer.phase.variant;
  const { history, outcome, cleanupFailed } = transfer;
  const last = history.at(-1);
  const supersededRefusal =
    outcome?.variant === "refused" && (outcome.value as Tagged).variant === "superseded";
  return (
    history.length > 0 &&
    history.length <= 8 &&
    history[0]?.phase.variant === "requested" &&
    // Phases compare whole, payload included (`entry.phase == self.phase`).
    JSON.stringify(last?.phase, replacer) === JSON.stringify(transfer.phase, replacer) &&
    history.every((entry, index) => {
      const before = history[index - 1];
      return (
        before === undefined ||
        (before.enteredAt <= entry.enteredAt &&
          allows(request, before.phase.variant, entry.phase.variant))
      );
    }) &&
    (phase !== "ended" || (outcome !== null && outcome.variant !== "pending")) &&
    (outcome === null || phase === "ended") &&
    (cleanupFailed === null ||
      (request === "move" &&
        (phase === "cancelling" || supersededRefusal) &&
        last !== undefined &&
        (cleanupFailed.at >= last.enteredAt || phase === "ended"))) &&
    (phase !== "cancelling" || outcome === null)
  );
}
const replacer = (_key: string, value: unknown) =>
  typeof value === "bigint" ? value.toString() : value;

const LIFECYCLE_OUTCOME: Readonly<Record<string, string>> = {
  register: "registration",
  import: "import",
  retire: "retirement",
};

export interface LeaseStop {
  readonly table: string;
  readonly row: string;
  readonly reason: string;
}

/** Every lease-table row as main's `validate_legacy_rows` reads it; each refusal names its row. */
export function checkLeaseTables(registry: ReadReply): LeaseStop[] {
  const stops: LeaseStop[] = [];
  const rows = (table: string): readonly ReadRow[] => registry.rows[table] ?? [];
  const each = (
    table: string,
    key: (row: ReadRow) => string,
    check: (row: ReadRow, at: string) => void,
  ) =>
    rows(table).forEach((row, index) => {
      const at = `${key(row)} [row ${index + 1}]`;
      try {
        check(row, at);
      } catch (error) {
        if (!(error instanceof Invalid)) throw error;
        stops.push({ table, row: at, reason: error.message });
      }
    });
  const label = (value: unknown): string => (typeof value === "string" ? value : "<missing>");

  const databases = new Map<string, { at: string; record: ReturnType<typeof databaseRecord> }>();
  each(
    "_fleet_sync_databases",
    (row) => label(row.database),
    (row, at) => {
      const database = scalar(uuidV7)(row.database, "database");
      const record = json(databaseRecord)(row.record, "record");
      if (record.database !== database) invalid("record", "identity differs from the row key");
      databases.set(database, { at, record });
    },
  );
  const leases = new Map<string, ReturnType<typeof registryLease>>();
  // A lease that fails to decode is already refused; its record's grant check would only repeat it.
  const undecodedLeases = new Set(rows("_fleet_sync_leases").map((row) => label(row.database)));
  each(
    "_fleet_sync_leases",
    (row) => label(row.database),
    (row) => {
      const database = scalar(uuidV7)(row.database, "database");
      const record = json(registryLease)(row.record, "record");
      if (record.database !== database) invalid("record", "identity differs from the row key");
      if (record.expires_at < record.last_renewed_at)
        invalid("record.expires_at", "before last_renewed_at");
      leases.set(database, record);
      undecodedLeases.delete(database);
    },
  );
  // DatabaseRecord::validate_grant.
  for (const [database, { at, record }] of databases) {
    if (undecodedLeases.has(database)) continue;
    const grant = leases.get(database);
    const lifecycle = record.lifecycle.variant;
    if (
      (grant !== undefined &&
        (grant.database !== record.database || grant.epoch !== record.revision.epoch)) ||
      (grant === undefined && lifecycle === "registered") ||
      (grant !== undefined && lifecycle === "unheld")
    ) {
      stops.push({
        table: "_fleet_sync_databases",
        row: at,
        reason:
          "registry revision grant: the lease and the record's revision or lifecycle disagree",
      });
    }
  }
  each(
    "_fleet_sync_epoch_floor",
    (row) => label(row.database),
    (row) => {
      scalar(uuidV7)(row.database, "database");
      storedEpoch(row.epoch, "epoch");
    },
  );
  each(
    "_fleet_sync_epoch_history",
    (row) => `${label(row.database)}/${label(row.epoch)}`,
    (row) => {
      const database = scalar(uuidV7)(row.database, "database");
      const stored = storedEpoch(row.epoch, "epoch");
      const record = json(epochHistory)(row.record, "record");
      // registry_storage.rs decode_history.
      if (record.database !== database || record.epoch !== stored)
        invalid("record", "identity differs from the row key");
      if (record.ended_at !== null && record.ended_at.epoch > stored)
        invalid("record.ended_at", "ends in a later epoch");
    },
  );
  each(
    "_fleet_sync_renewal_attempts",
    (row) => `${label(row.database)}/${label(row.attempt)}`,
    (row) => {
      const database = scalar(uuidV7)(row.database, "database");
      const attempt = scalar(uuidV7)(row.attempt, "attempt");
      const record = json(renewalAttempt)(row.record, "record");
      if (record.request.database !== database || record.request.attempt !== attempt)
        invalid("record", "identity differs from the row key");
    },
  );
  each(
    "_fleet_sync_takeover_previews",
    (row) => label(row.token),
    (row) => {
      const token = scalar(uuidV7)(row.token, "token");
      const database = scalar(uuidV7)(row.database, "database");
      const record = json(previewRecord)(row.record, "record");
      if (record.preview.token !== token || record.database !== database)
        invalid("record", "identity differs from the row key");
    },
  );
  each(
    "_fleet_sync_lifecycle_attempts",
    (row) => label(row.attempt),
    (row) => {
      const attempt = scalar(uuidV7)(row.attempt, "attempt");
      const database = scalar(uuidV7)(row.database, "database");
      const intent = json(lifecycleIntent)(row.intent, "intent");
      const request = intent.value as Payload;
      if (request.attempt !== attempt || request.database !== database)
        invalid("intent", "identity differs from the row key");
      const outcome = optionalJson(lifecycleOutcome)(row.outcome, "outcome");
      if (
        outcome !== null &&
        outcome.variant !== "failed" &&
        outcome.variant !== LIFECYCLE_OUTCOME[intent.variant]
      )
        invalid("outcome", "outcome kind differs from the intent");
    },
  );
  const admissions = new Map<string, Tagged>();
  each(
    "_fleet_sync_transfer_admissions",
    (row) => label(row.attempt),
    (row) => {
      const attempt = scalar(uuidV7)(row.attempt, "attempt");
      const operation = json(uuidV7)(row.operation, "operation");
      const request = json(holderChangeRequest)(row.request, "request");
      json(actorRef)(row.requested_by, "requested_by");
      json(syncTime)(row.admitted_at, "admitted_at");
      json(option(schemaSupport))(row.target_schema, "target_schema");
      const followUp = optionalJson(uuidV7)(row.follow_up, "follow_up");
      if (payload(request).attempt !== attempt || followUp === operation)
        invalid("request", "identity differs from the row key");
      admissions.set(attempt, request);
    },
  );
  each(
    "_fleet_sync_transfers",
    (row) => label(row.attempt),
    (row) => {
      const attempt = scalar(uuidV7)(row.attempt, "attempt");
      const database = scalar(uuidV7)(row.database, "database");
      const request = admissions.get(attempt) ?? invalid("attempt", "has no admission");
      json(option(epoch))(row.intent, "reserved epoch");
      if (payload(request).database !== database) invalid("request", "names another database");
      const outcome = optionalJson(holderChangeAnswer)(row.outcome, "outcome");
      const phase = json(holderChangePhase)(row.phase, "phase");
      const history = json(vec(phaseEntry))(row.phase_history, "phase_history");
      const cleanupFailed = optionalJson(cleanupFailure)(row.cleanup_failed, "cleanup_failed");
      if (!validTransfer({ request, outcome, phase, history, cleanupFailed }))
        invalid("phase_history", "holder-change phase history main refuses");
    },
  );
  each(
    "_fleet_sync_transfer_cancels",
    (row) => label(row.attempt),
    (row) => {
      scalar(uuidV7)(row.attempt, "attempt");
      json(cancelRecord)(row.record, "record");
    },
  );
  each(
    "_fleet_sync_schema_support",
    (row) => label(row.peer),
    (row) => {
      const peer = scalar(uuidV7)(row.peer, "peer");
      const record = json(reportSchemaSupport)(row.record, "record");
      if (record.peer !== peer) invalid("record", "identity differs from the row key");
    },
  );
  return stops;
}

/** fleet-sync-receiver copy_contract.rs `CopyRecord`, as its `validate` accepts it. */
const copyStatus = struct(
  {
    database: uuidV7,
    role: unitEnum(["authoritative", "follower"]),
    fence: epoch,
    holder: option(uuidV7),
    start: position,
    acked: position,
    schema: schemaIdentity,
  },
  true,
);
const copyRecord = adjacentlyTagged("state", "copy", {
  starting: fields({ status: copyStatus, install_id: uuidV7 }),
  seeding: fields({ status: copyStatus, expected_position: option(position) }),
  following: newtype(copyStatus),
  retired: fields({ database: uuidV7, retired_at: syncTime }),
});

export type CopyStatus = ReturnType<typeof copyStatus>;
export type CopyRecord =
  | { readonly state: "starting" | "seeding" | "following"; readonly status: CopyStatus }
  | { readonly state: "retired"; readonly database: string };

const before = (a: { epoch: bigint; sequence: bigint }, b: { epoch: bigint; sequence: bigint }) =>
  a.epoch < b.epoch || (a.epoch === b.epoch && a.sequence < b.sequence);

/** Decodes and validates a stored Copy record, or throws `Invalid` naming what main refuses. */
export function decodeCopyRecord(stored: unknown): CopyRecord {
  const record = json(copyRecord)(stored, "_fleet_sync_copy.record");
  if (record.variant === "retired") {
    const { database } = record.value as { database: string };
    if (database === "00000000-0000-0000-0000-000000000000") invalid("retired", "nil database");
    return { state: "retired", database };
  }
  const status = (
    record.variant === "following" ? record.value : (record.value as Payload).status
  ) as CopyStatus;
  if (record.variant === "starting" && status.role !== "follower")
    invalid("starting copy marker", "a starting copy must be a follower");
  // copy_contract.rs validate_copy_status.
  if (
    status.fence < status.acked.epoch ||
    before(status.acked, status.start) ||
    (status.holder === null && status.fence !== 1n)
  )
    invalid("copy state", "fence, start, acknowledged position or holder main refuses");
  return { state: record.variant as "starting" | "seeding" | "following", status };
}
