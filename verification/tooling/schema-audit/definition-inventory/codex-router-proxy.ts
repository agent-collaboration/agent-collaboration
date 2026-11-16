import type { InventoryRow } from "../schema-audit-types.ts";

/**
 * No rows: the model proxy carries OpenAI Responses and Anthropic Messages traffic (and serves the
 * Router GET /healthz body), all outside Project 2's protocols (see the ledger entries).
 */
export const rows: InventoryRow[] = [];
