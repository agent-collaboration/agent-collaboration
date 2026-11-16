import type { InventoryRow } from "../schema-audit-types.ts";

/**
 * No rows: the board-mail recipients built with json! in fleet_mail_observer_scenario.rs:102 and
 * sender_revocation_scenario.rs:54 are test input decoded by the canonical SessionRef type, not a
 * second schema (Lead ruling 01a1e2c1); the rest of the crate is central-only conformance code.
 */
export const rows: InventoryRow[] = [];
