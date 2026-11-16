import type { InventoryRow } from "../schema-audit-types.ts";

/**
 * No rows: host_command and presentation/host.rs carry the Host operator socket (OperatorRequest,
 * OperatorFrame, HostProgress, TerminalClassification), outside Project 2's protocols (see the
 * ledger entries).
 */
export const rows: InventoryRow[] = [];
