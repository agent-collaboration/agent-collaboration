/** Replay original D4 socket bytes; this measures a retained trace and never qualifies a product target. */
import { lstat, readFile } from "node:fs/promises";
import { z } from "zod";
import { decode, encode } from "../../../exact-json.ts";
import { WindowByteAudit, type WindowByteSnapshot } from "./window-byte-audit.ts";

const connection = z.string().min(1).max(256);
const recordSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("opened"), connection }).strict(),
  z.object({ kind: z.literal("ended"), connection }).strict(),
  z
    .object({
      kind: z.literal("publicationSubject"),
      connection,
      sendId: z.uuidv7(),
      subject: z.uuidv7(),
    })
    .strict(),
  z.object({ kind: z.literal("server"), connection, bytes: z.string().min(1) }).strict(),
  z.object({ kind: z.literal("client"), connection, bytes: z.string().min(1) }).strict(),
]);
const traceBound = 32 * 1024 * 1024;

export function replayWindowBytes(contents: string): WindowByteSnapshot {
  if (Buffer.byteLength(contents) > traceBound) throw new Error("window trace exceeds 32 MiB");
  const audit = new WindowByteAudit();
  for (const line of contents.split("\n")) {
    if (line.trim() === "") continue;
    const record = recordSchema.parse(decode(line));
    switch (record.kind) {
      case "opened":
        audit.opened(record.connection);
        break;
      case "ended":
        audit.ended(record.connection);
        break;
      case "publicationSubject":
        audit.bindPublicationSubject(record.connection, record.sendId, record.subject);
        break;
      case "server":
      case "client": {
        const bytes = Buffer.from(record.bytes, "base64");
        if (bytes.toString("base64") !== record.bytes) throw new Error("noncanonical byte record");
        if (record.kind === "server") audit.server(record.connection, bytes);
        else audit.client(record.connection, bytes);
        break;
      }
      default:
        assertNever(record);
    }
  }
  audit.requireWindow();
  return audit.snapshot();
}
function assertNever(value: never): never {
  throw new Error(`unhandled window trace record: ${encode(value)}`);
}
async function main(): Promise<void> {
  const arguments_ = process.argv.slice(2);
  if (arguments_.length === 1 && arguments_[0] === "--help") {
    process.stdout.write(
      "D4 window byte reader\nUsage: node verification/observations/central/d4-central-engine/window-readback.ts TRACE.jsonl\nReads an owner-private original byte transcript, preserving per-physical-socket order.\nRows: {kind:opened|ended,connection} or {kind:server|client,connection,bytes:canonical-base64}.\nStatus sends also require a preceding {kind:publicationSubject,connection,sendId:UUIDv7,subject:UUIDv7} from actual conformance evidence.\nRequires real Hello/Welcome, complete valid bytes both ways and a nonempty owed-request observation; rejects a peak above 32.\nOutput is a retained-byte measurement only. It supplies no source/build binding, durable-row comparison, runtime journey, hour/wake or product sensitivity credit.\nNo processes, network, model inputs or state writes.\n",
    );
    return;
  }
  if (arguments_.length !== 1 || arguments_[0] === undefined)
    throw new Error("one TRACE.jsonl path required; see --help");
  const info = await lstat(arguments_[0]);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0 ||
    info.size > traceBound
  )
    throw new Error("bounded owner-private regular byte transcript required");
  const snapshot = replayWindowBytes(await readFile(arguments_[0], "utf8"));
  process.stdout.write(
    `${encode({ scope: "retained-byte measurement only", runtimeQualified: false, snapshot })}\n`,
  );
}
if (import.meta.main)
  main().catch((error: unknown): void => {
    process.stderr.write(`${String(error)}\n`);
    process.exitCode = 1;
  });
