import { chmodSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { parseArgs } from "node:util";
import { z } from "zod";
import { evidenceJson, prettyEvidenceJson } from "../evidence-json.ts";
import {
  exactIntegerSchema,
  readObservedJson,
  storedInteractionSchema,
  type JsonObject,
} from "./evidence-models.ts";
import { compareReopen } from "./callback-oracles.ts";

export {
  callbackPrompt,
  compareReopen,
  decodeRpcBody,
  questionResponseFor,
  requireAdvertisedSettings,
  requireCallbackCapture,
} from "./callback-oracles.ts";

export function readSnapshot(database: string, requestIds: readonly string[]): JsonObject {
  const path = realpathSync(database);
  const connection = new DatabaseSync(path, { readOnly: true });
  try {
    connection.exec("BEGIN");
    const revisionQuery = connection.prepare(
      "SELECT metadata_id, revision FROM interaction_history_revision",
    );
    revisionQuery.setReadBigInts(true);
    const revisions = z
      .array(z.object({ metadata_id: exactIntegerSchema, revision: exactIntegerSchema }))
      .parse(revisionQuery.all());
    const revision = revisions[0];
    if (
      revisions.length !== 1 ||
      revision === undefined ||
      revision.metadata_id !== 1n ||
      revision.revision < 0n
    )
      throw new Error("invalid authoritative revision");
    const records: JsonObject[] = [];
    const query = connection.prepare(
      "SELECT request_id,record_json,created_at FROM typed_interaction_history WHERE request_id=?",
    );
    for (const requestId of requestIds) {
      const raw = query.get(requestId);
      if (raw === undefined) throw new Error("scoped interaction missing");
      const row = z
        .object({ request_id: z.string(), record_json: z.string(), created_at: z.string() })
        .parse(raw);
      const record = readObservedJson(row.record_json);
      const view = storedInteractionSchema.parse(record);
      if (view.request === null || view.request.requestId !== requestId)
        throw new Error("request identity mismatch");
      records.push({ requestId: row.request_id, record, createdAt: row.created_at });
    }
    return { database: path, revision: revision.revision, records };
  } finally {
    connection.close();
  }
}

export function main(): void {
  if (process.argv.slice(2).some((argument) => argument === "--help" || argument === "-h")) {
    console.log(
      "usage: interaction-readback.ts --database DATABASE --request-id ID [--request-id ID ...] [--before SNAPSHOT] --output OUTPUT",
    );
    return;
  }
  const { values } = parseArgs({
    options: {
      database: { type: "string" },
      "request-id": { type: "string", multiple: true },
      before: { type: "string" },
      output: { type: "string" },
    },
  });
  if (
    values.database === undefined ||
    values["request-id"] === undefined ||
    values.output === undefined
  )
    throw new Error("--database, --request-id and --output are required");
  const snapshot = readSnapshot(values.database, values["request-id"]);
  if (values.before !== undefined)
    console.log(
      evidenceJson(compareReopen(readObservedJson(readFileSync(values.before)), snapshot)),
    );
  writeFileSync(values.output, `${prettyEvidenceJson(snapshot)}\n`, { flag: "wx", mode: 0o600 });
  chmodSync(values.output, 0o600);
}

if (import.meta.main) main();
