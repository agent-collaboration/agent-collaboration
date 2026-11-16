import type { InventoryRow } from "../schema-audit-types.ts";

/**
 * No rows: live_quota.rs StoredAuth/StoredTokens read the Codex-owned auth.json file, a local
 * credential file and not one of the four audited protocols (see the ledger entry).
 */
export const rows: InventoryRow[] = [];
