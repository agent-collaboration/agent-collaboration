// Reads stored roles and operation states from the Host's owning provider store, read-only.
// No public view exposes a Session's creator or Approver, so this is the only readback of them.
// Three identities stay distinct and are never inferred from one another (Lead push 01a1a95c):
// the Session's stored creator/Approver (provider_session_records), an operation's initiating
// requester (checked at admission and NOT persisted: provider_operations has no requester
// column, provider_operation_store.rs:120-135) and a broker interaction's requester.
//
// While the Host is live the database is in WAL mode, so it is opened normally read-only (a
// shared read lock, no data write); after the Host has stopped and its WAL is gone, callers may
// open it the same way. `immutable=1` would ignore the WAL and miss fresh rows, so it is unused.
import { DatabaseSync } from "node:sqlite";
import { z } from "zod";
import { decode } from "../../../exact-json.ts";
import { sessionRefSchema, type SessionRef } from "../observations/observation-models.ts";

// ProviderIdentity is untagged: a Session is bare SessionRef JSON, a Human is {"humanId": ...}
// (collaboration-protocol provider_identity.rs:8-16).
const storedIdentitySchema = z.union([
  z
    .strictObject({ humanId: z.string().min(1) })
    .transform((value) => ({ kind: "human" as const, humanId: value.humanId })),
  sessionRefSchema.transform((session) => ({ kind: "session" as const, session })),
]);
export type StoredIdentity = z.infer<typeof storedIdentitySchema>;

export interface StoredSessionRoles {
  readonly createdBy: StoredIdentity;
  readonly approver: StoredIdentity;
}

export interface StoredOperation {
  readonly operationId: string;
  readonly operationKind: string;
  readonly stage: string;
  readonly effect: string;
  readonly reconciliation: string;
  readonly terminal: boolean;
}

const roleRowSchema = z.object({ created_by_json: z.string(), approver_json: z.string() });
const operationRowSchema = z.object({
  operation_id: z.string(),
  operation_kind: z.string(),
  stage: z.string(),
  effect: z.string(),
  reconciliation_state: z.string(),
  terminal_at_ms: z.union([z.number(), z.bigint()]).nullable(),
});

export class ProviderStoreReadback {
  private readonly database: DatabaseSync;
  constructor(databasePath: string) {
    this.database = new DatabaseSync(databasePath, { readOnly: true });
  }

  /** The target's stored roles, or null when the store has no record of it. */
  sessionRoles(target: SessionRef): StoredSessionRoles | null {
    const row = this.database
      .prepare(
        "SELECT created_by_json, approver_json FROM provider_session_records WHERE target_service_id = ? AND target_endpoint_id = ? AND target_session_id = ?",
      )
      .get(target.endpoint.serviceId, target.endpoint.endpointId, target.sessionId);
    if (row === undefined) return null;
    const parsed = roleRowSchema.parse(row);
    return {
      createdBy: storedIdentitySchema.parse(decode(parsed.created_by_json)),
      approver: storedIdentitySchema.parse(decode(parsed.approver_json)),
    };
  }

  /** Every operation recorded against the target, oldest first. */
  operations(target: SessionRef): StoredOperation[] {
    return this.database
      .prepare(
        "SELECT operation_id, operation_kind, stage, effect, reconciliation_state, terminal_at_ms FROM provider_operations WHERE target_service_id = ? AND target_endpoint_id = ? AND target_session_id = ? ORDER BY admitted_at_ms, operation_id",
      )
      .all(target.endpoint.serviceId, target.endpoint.endpointId, target.sessionId)
      .map((row): StoredOperation => {
        const parsed = operationRowSchema.parse(row);
        return {
          operationId: parsed.operation_id,
          operationKind: parsed.operation_kind,
          stage: parsed.stage,
          effect: parsed.effect,
          reconciliation: parsed.reconciliation_state,
          terminal: parsed.terminal_at_ms !== null,
        };
      });
  }

  /** Operations anywhere in the store that have not reached a terminal record. */
  nonTerminalCount(): number {
    const row = z
      .object({ count: z.union([z.number(), z.bigint()]) })
      .parse(
        this.database
          .prepare("SELECT COUNT(*) AS count FROM provider_operations WHERE terminal_at_ms IS NULL")
          .get(),
      );
    return Number(row.count);
  }

  close(): void {
    this.database.close();
  }
}

/** Whether a stored role is exactly the identity the run declared. */
export function storedIdentityIs(stored: StoredIdentity, declared: StoredIdentity): boolean {
  if (stored.kind === "human" && declared.kind === "human")
    return stored.humanId === declared.humanId;
  if (stored.kind === "session" && declared.kind === "session")
    return (
      stored.session.sessionId === declared.session.sessionId &&
      stored.session.endpoint.serviceId === declared.session.endpoint.serviceId &&
      stored.session.endpoint.endpointId === declared.session.endpoint.endpointId
    );
  return false;
}
