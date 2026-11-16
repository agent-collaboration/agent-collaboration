import type { InventoryRow } from "../schema-audit-types.ts";

/**
 * No rows: RouterCompatibility (router_compatibility.rs:11) is the GET /healthz body and AuditEvent
 * (audit.rs:85) the audit JSONL line; neither is one of the four audited protocols (see the ledger).
 */
export const rows: InventoryRow[] = [];
