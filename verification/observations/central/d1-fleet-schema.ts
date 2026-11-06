/** Used fields of the actual FleetSchemaObject wire; original replies stay authoritative. */
import { z } from "zod";

export const fleetSchemaObjects = z.array(
  z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("table"), name: z.string().min(1), definition: z.string().min(1) }),
    z.object({
      kind: z.literal("index"),
      name: z.string().min(1),
      table: z.string().min(1),
      definition: z.string().nullable(),
    }),
  ]),
);
export type FleetSchemaObject = z.infer<typeof fleetSchemaObjects>[number];

export interface FleetSchemaReadback {
  readonly raw: unknown;
  readonly objects: readonly FleetSchemaObject[];
}

export function parseFleetSchemaReadback(raw: unknown): FleetSchemaReadback {
  const objects = fleetSchemaObjects.parse(raw);
  const names = objects.map((object): string => object.name);
  if (objects.length === 0 || new Set(names).size !== names.length)
    throw new Error("Fleet schema inventory is empty or duplicates an object name");
  if (!objects.some((object): boolean => object.kind === "table"))
    throw new Error("Fleet schema inventory has no tables");
  for (const object of objects)
    if (
      object.kind === "index" &&
      !objects.some((table): boolean => table.kind === "table" && table.name === object.table)
    )
      throw new Error("Fleet index refers to an absent table");
  return { raw, objects };
}
