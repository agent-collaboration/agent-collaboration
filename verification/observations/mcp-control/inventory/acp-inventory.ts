// The maintained ACP-to-MCP coverage inventory and its checks. It is verification data, never a
// runtime registry (mcp-facade.md, Capability inventory). The locked ACP set is a pure function
// of the pinned agent-client-protocol-schema version, so the permanent test checks the pin
// against Cargo.lock; `run.ts --check` re-derives the names from that version's real source and
// compares them, so an added, removed or newly gated member fails coverage instead of vanishing.
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { z } from "zod";
import { decodeAuthoredJson } from "../../../exact-json.ts";

const rowSchema = z.strictObject({
  name: z.string().min(1),
  kind: z.enum(["method", "notification", "sessionUpdate", "extension"]),
  direction: z.enum(["clientToAgent", "agentToClient", "both"]),
  gate: z.string().min(1).nullable(),
  disposition: z.enum(["qualified", "represented", "hostOwned", "unsupported", "compileGated"]),
  tools: z.array(z.string().min(1)),
  proof: z.string().min(1).nullable(),
  note: z.string(),
});
export const acpInventorySchema = z.strictObject({
  schemaCrate: z.literal("agent-client-protocol-schema"),
  schemaVersion: z.string().min(1),
  protocol: z.literal("v1"),
  enabledFeatures: z.array(z.string()),
  dispositions: z.record(z.string(), z.string()),
  rows: z.array(rowSchema),
  facadeGaps: z.array(z.string().min(1)).min(1),
});
export type AcpInventory = z.infer<typeof acpInventorySchema>;
export type InventoryRow = AcpInventory["rows"][number];

export interface LockedName {
  readonly name: string;
  readonly kind: "method" | "notification" | "sessionUpdate";
  readonly gate: string | null;
}

const here = dirname(fileURLToPath(import.meta.url));
export function loadAcpInventory(path = join(here, "acp-inventory.json")): AcpInventory {
  return acpInventorySchema.parse(decodeAuthoredJson(readFileSync(path, "utf8")));
}

/** The locked schema version Cargo.lock pins, or null when the crate is absent. */
export function lockedSchemaVersion(cargoLock: string, crate: string): string | null {
  const block = cargoLock
    .split("[[package]]")
    .find((entry): boolean => entry.includes(`name = "${crate}"\n`));
  return block?.match(/\nversion = "([^"]+)"/)?.[1] ?? null;
}

/** Structural defects: duplicates, gated rows claiming support, positive rows without proof. */
export function inventoryDefects(inventory: AcpInventory): string[] {
  const defects: string[] = [];
  const seen = new Set<string>();
  for (const row of inventory.rows) {
    const key = `${row.kind}:${row.name}`;
    if (seen.has(key)) defects.push(`${row.name} is listed twice`);
    seen.add(key);
    const gatedOff = row.gate !== null && !inventory.enabledFeatures.includes(row.gate);
    if (gatedOff !== (row.disposition === "compileGated"))
      defects.push(`${row.name}: compile gate and disposition disagree`);
    if (row.disposition === "qualified" && (row.proof === null || row.tools.length === 0))
      defects.push(`${row.name} claims qualification without a tool and a deciding case`);
    if (row.disposition !== "qualified" && row.proof !== null)
      defects.push(`${row.name} names a proof but does not claim qualification`);
    if (["unsupported", "compileGated"].includes(row.disposition) && row.tools.length > 0)
      defects.push(`${row.name} is not offered but names MCP tools`);
  }
  return defects;
}

/** Locked members the inventory omits, rows it invents, and gates that changed. */
export function lockedCoverageDefects(
  inventory: AcpInventory,
  locked: readonly LockedName[],
): string[] {
  const defects: string[] = [];
  const rows = new Map(
    inventory.rows
      .filter((row) => row.kind !== "extension")
      .map((row) => [`${row.kind}:${row.name}`, row]),
  );
  for (const member of locked) {
    const row = rows.get(`${member.kind}:${member.name}`);
    if (row === undefined)
      defects.push(`locked ${member.kind} ${member.name} has no inventory row`);
    else if (row.gate !== member.gate)
      defects.push(`${member.name} gate is ${member.gate ?? "none"}, not ${row.gate ?? "none"}`);
    rows.delete(`${member.kind}:${member.name}`);
  }
  for (const row of rows.values())
    defects.push(`${row.kind} ${row.name} is not in the locked schema`);
  return defects;
}

/** Tools a represented or qualified row names that the live `tools/list` does not offer. */
export function toolCoverageDefects(
  inventory: AcpInventory,
  offeredTools: readonly string[],
): string[] {
  const offered = new Set(offeredTools);
  return inventory.rows.flatMap((row): string[] =>
    row.tools
      .filter((tool) => !offered.has(tool))
      .map((tool) => `${row.name} names ${tool}, which tools/list does not offer`),
  );
}

const NOTIFICATIONS_NAMED_AS_METHODS = new Set(["session/cancel", "$/cancel_request"]);
const MODULE_GATES: Readonly<Record<string, string>> = {
  "mcp.rs": "unstable_mcp_over_acp",
  "nes.rs": "unstable_nes",
};

/** Method and notification names declared in the schema's v1 source, with their feature gates. */
export function extractLockedNames(v1SourceDirectory: string): LockedName[] {
  const names = new Map<string, LockedName>();
  for (const file of readdirSync(v1SourceDirectory)
    .filter((entry) => entry.endsWith(".rs"))
    .sort()) {
    const lines = readFileSync(join(v1SourceDirectory, file), "utf8").split("\n");
    let gate: string | null = null;
    for (const line of lines) {
      const cfg = line.match(/^\s*#\[cfg\(feature = "([a-z0-9_]+)"\)\]/);
      if (cfg !== null) {
        gate = cfg[1] ?? null;
        continue;
      }
      const constant = line.match(/const [A-Z0-9_]+_(METHOD_NAME|NOTIFICATION): &str = "([^"]+)";/);
      if (constant !== null && constant[2] !== undefined) {
        // session/cancel and $/cancel_request are notifications the schema names *_METHOD_NAME.
        const kind =
          constant[1] === "NOTIFICATION" || NOTIFICATIONS_NAMED_AS_METHODS.has(constant[2])
            ? "notification"
            : "method";
        names.set(constant[2], {
          name: constant[2],
          kind,
          gate: gate ?? MODULE_GATES[file] ?? null,
        });
      }
      if (!/^\s*(\/\/|#\[)/.test(line)) gate = null;
    }
  }
  return [...names.values()];
}

/** `SessionUpdate` variants (snake_case on the wire) with their feature gates. */
export function extractSessionUpdates(clientSource: string): LockedName[] {
  const start = clientSource.indexOf("pub enum SessionUpdate {");
  if (start < 0) throw new Error("SessionUpdate enum not found in the locked schema source");
  const body = clientSource.slice(start, clientSource.indexOf("\n}", start));
  const variants: LockedName[] = [];
  let gate: string | null = null;
  for (const line of body.split("\n").slice(1)) {
    const cfg = line.match(/^\s*#\[cfg\(feature = "([a-z0-9_]+)"\)\]/);
    if (cfg !== null) {
      gate = cfg[1] ?? null;
      continue;
    }
    const variant = line.match(/^\s{4}([A-Z][A-Za-z]+)\(/);
    if (variant?.[1] !== undefined) {
      const name = variant[1].replace(/([a-z])([A-Z])/g, "$1_$2").toLowerCase();
      variants.push({ name, kind: "sessionUpdate", gate });
    }
    if (!/^\s*(\/\/|#\[)/.test(line)) gate = null;
  }
  return variants;
}
