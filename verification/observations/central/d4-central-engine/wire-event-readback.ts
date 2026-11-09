/** Read private retained wire/S1 artifacts and report their joined window measurement. */
import { lstat, readFile } from "node:fs/promises";
import { z } from "zod";
import { decode, encode } from "../../../exact-json.ts";
import { parseD4StderrLine, type D4EventRecord } from "./event-observation.ts";
import { joinWireEvents, wireConnectionBindingSchema } from "./wire-event-join.ts";
import { replayWindowBytes } from "./window-readback.ts";

const inputBound = 32 * 1024 * 1024;
async function readPrivateArtifact(path: string): Promise<string> {
  const info = await lstat(path);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0 ||
    info.size > inputBound
  )
    throw new Error("bounded owner-private regular capture artifact required");
  return readFile(path, "utf8");
}

export async function readJoinedWindow(paths: {
  readonly wire: string;
  readonly events: string;
  readonly connections: string;
}): Promise<unknown> {
  const [trace, eventLines, connectionText] = await Promise.all([
    readPrivateArtifact(paths.wire),
    readPrivateArtifact(paths.events),
    readPrivateArtifact(paths.connections),
  ]);
  const events: D4EventRecord[] = [];
  for (const line of eventLines.split("\n")) {
    const record = parseD4StderrLine(line);
    if (record !== undefined && "centralEvent" in record) events.push(record.centralEvent);
  }
  const joined = joinWireEvents({
    trace,
    events,
    connections: z.array(wireConnectionBindingSchema).parse(decode(connectionText)),
  });
  return {
    scope: "retained-byte/S1 join and window measurement only",
    runtimeQualified: false,
    matchedSends: joined.matchedSends,
    sourceSendsNotOnWire: joined.sourceSendsNotOnWire,
    snapshot: replayWindowBytes(joined.trace),
  };
}

async function main(): Promise<void> {
  const arguments_ = process.argv.slice(2);
  if (arguments_.length === 1 && arguments_[0] === "--help") {
    process.stdout.write(
      "D4 original-wire/S1 join\nUsage: node verification/observations/central/d4-central-engine/wire-event-readback.ts WIRE.jsonl EVENTS.stderr CONNECTIONS.json\nAll three inputs must be bounded owner-private regular files.\nWIRE is the original unbound window trace. EVENTS retains the actual S1 centralEvent stderr records.\nCONNECTIONS is an array of {physical:capture-label,connection:UUIDv7,session:UUIDv7} from actual fixture/platform evidence.\nJoins exact original payloads by send UUID without changing wire order, then measures distinct pending attempt slots (maximum 32).\nOutputs a measurement with runtimeQualified:false, never capture provenance, complete event-sequence or product proof.\nNo processes, network, model inputs or state writes.\n",
    );
    return;
  }
  const [wire, events, connections] = arguments_;
  if (
    arguments_.length !== 3 ||
    wire === undefined ||
    events === undefined ||
    connections === undefined
  )
    throw new Error("WIRE.jsonl EVENTS.stderr CONNECTIONS.json required; see --help");
  process.stdout.write(`${encode(await readJoinedWindow({ wire, events, connections }))}\n`);
}
if (import.meta.main)
  main().catch((error: unknown): void => {
    process.stderr.write(`${String(error)}\n`);
    process.exitCode = 1;
  });
