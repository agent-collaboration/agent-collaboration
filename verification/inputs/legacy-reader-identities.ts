import { z } from "zod";
import { identitySchema, type Identity } from "../declaration.ts";
import { decode } from "../exact-json.ts";
import { command } from "../machines/owned-process.ts";
import type { SealedLegacyCopy } from "./sealed-legacy-copy.ts";

const rowsSchema = z.array(
  z.discriminatedUnion("kind", [
    z.object({
      identity_key: z.string(),
      kind: z.literal("human"),
      human_id: z.string(),
      service_id: z.null(),
      endpoint_id: z.null(),
      session_id: z.null(),
    }),
    z.object({
      identity_key: z.string(),
      kind: z.literal("session"),
      human_id: z.null(),
      service_id: z.string(),
      endpoint_id: z.string(),
      session_id: z.string(),
    }),
  ]),
);
export async function readLegacyReaderIdentities(
  copy: SealedLegacyCopy,
): Promise<ReadonlyMap<string, Identity>> {
  await copy.verify();
  try {
    const result = await command("/usr/bin/sqlite3", [
      "-readonly",
      "-json",
      copy.input.path,
      "SELECT identity_key,kind,service_id,endpoint_id,session_id,human_id FROM board_identities ORDER BY identity_key",
    ]);
    if (result.exit !== 0) throw new Error("sealed legacy reader identity read failed");
    const identities = new Map<string, Identity>();
    for (const row of rowsSchema.parse(decode(result.stdout))) {
      if (identities.has(row.identity_key))
        throw new Error("sealed legacy reader identity repeated");
      const identity =
        row.kind === "human"
          ? { kind: "human", humanId: row.human_id }
          : {
              kind: "session",
              session: {
                endpoint: { serviceId: row.service_id, endpointId: row.endpoint_id },
                sessionId: row.session_id,
              },
            };
      identities.set(row.identity_key, identitySchema.parse(identity));
    }
    return identities;
  } finally {
    await copy.verify();
  }
}
