/** Read only the owned Host's backend/observer journal facts, never thread contents. */
import { existsSync, realpathSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { z } from "zod";
import {
  endpointViewSchema,
  exactIntegerSchema,
  generationViewSchema,
  readObservedJson,
} from "../shared/evidence-models.ts";

const backendObservationSchema = z
  .looseObject({
    observedAt: z.string(),
    source: z.enum(["hostLifecycle", "observerLifecycle"]),
    scope: z.looseObject({
      endpoint: endpointViewSchema,
      generation: generationViewSchema.nullable(),
      observerId: z.string(),
    }),
    subject: z.looseObject({ kind: z.literal("backend") }),
    change: z.discriminatedUnion("kind", [
      z.looseObject({
        kind: z.literal("backendStatus"),
        status: z.enum(["starting", "ready", "unavailable", "failed"]),
      }),
      z.looseObject({ kind: z.literal("coverageRestored") }),
      z.looseObject({ kind: z.literal("coverageLost") }),
    ]),
  })
  .superRefine((observation, context): void => {
    const backend = observation.change.kind === "backendStatus";
    if (observation.source !== (backend ? "hostLifecycle" : "observerLifecycle"))
      context.addIssue({ code: "custom", message: "journal fact has the wrong source" });
    if (
      observation.scope.generation === null &&
      (observation.change.kind === "coverageRestored" ||
        (observation.change.kind === "backendStatus" && observation.change.status === "ready"))
    )
      context.addIssue({
        code: "custom",
        message: "ready/connected journal fact lacks a generation",
      });
  });
const journalRecordSchema = z.object({
  sequence: exactIntegerSchema,
  observation: backendObservationSchema,
});
type JournalRecord = z.infer<typeof journalRecordSchema>;

export interface ObserverJournalInput {
  readonly routerRoot: string;
  readonly serviceId: string;
  readonly serviceEpoch: string;
  readonly afterSequence?: bigint;
}

interface ObserverEvidence {
  readonly database: string;
  readonly afterSequence: bigint;
  readonly throughSequence: bigint;
  readonly records: readonly JournalRecord[];
}

export type ObserverJournalReceipt = ObserverEvidence &
  (
    | { readonly kind: "waiting"; readonly reason: string }
    | {
        readonly kind: "connected";
        readonly backend: JournalRecord;
        readonly observer: JournalRecord;
      }
  );

export function readObserverJournal(input: ObserverJournalInput): ObserverJournalReceipt {
  const database = join(input.routerRoot, "agent-communication/session-registry.sqlite");
  const afterSequence = input.afterSequence ?? 0n;
  if (!existsSync(database))
    return {
      kind: "waiting",
      reason: "lifecycle journal not created",
      database,
      afterSequence,
      throughSequence: 0n,
      records: [],
    };
  const expected = join(
    realpathSync(input.routerRoot),
    "agent-communication/session-registry.sqlite",
  );
  if (realpathSync(database) !== expected)
    throw new Error("lifecycle journal resolves outside the owned Router root");
  const connection = new DatabaseSync(database, { readOnly: true });
  try {
    connection.exec("BEGIN");
    const metadataQuery = connection.prepare(
      "SELECT last_sequence FROM journal_metadata WHERE singleton=1",
    );
    metadataQuery.setReadBigInts(true);
    const metadata = z
      .object({ last_sequence: exactIntegerSchema.nonnegative() })
      .parse(metadataQuery.get());
    const query = connection.prepare(
      "SELECT sequence,observation_json FROM lifecycle_records WHERE sequence>? " +
        "AND json_extract(observation_json,'$.subject.kind')='backend' " +
        "AND json_extract(observation_json,'$.scope.endpoint.serviceId')=? " +
        "AND json_extract(observation_json,'$.scope.endpoint.endpointId')='codex-local' " +
        "ORDER BY sequence",
    );
    query.setReadBigInts(true);
    const rows = z
      .array(z.object({ sequence: exactIntegerSchema, observation_json: z.string() }))
      .parse(query.all(afterSequence, input.serviceId));
    const records = rows.map((row): JournalRecord =>
      journalRecordSchema.parse({
        sequence: row.sequence,
        observation: readObservedJson(row.observation_json),
      }),
    );
    const evidence = {
      database: expected,
      afterSequence,
      throughSequence: metadata.last_sequence,
      records,
    };
    const backend = records
      .filter((record) => record.observation.change.kind === "backendStatus")
      .at(-1);
    const observer = records
      .filter((record) => record.observation.change.kind !== "backendStatus")
      .at(-1);
    if (
      backend?.observation.change.kind !== "backendStatus" ||
      backend.observation.change.status !== "ready"
    )
      return { ...evidence, kind: "waiting", reason: "latest backend fact is not ready" };
    const backendGeneration = backend.observation.scope.generation;
    const observerGeneration = observer?.observation.scope.generation;
    if (
      backendGeneration === null ||
      backendGeneration.serviceEpoch !== input.serviceEpoch ||
      observer?.observation.change.kind !== "coverageRestored" ||
      observerGeneration === null ||
      observerGeneration === undefined ||
      observerGeneration.serviceEpoch !== input.serviceEpoch ||
      observerGeneration.generation !== backendGeneration.generation
    )
      return { ...evidence, kind: "waiting", reason: "no current-generation observer coverage" };
    return { ...evidence, kind: "connected", backend, observer };
  } finally {
    connection.close();
  }
}
