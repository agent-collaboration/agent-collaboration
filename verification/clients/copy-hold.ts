import { createHash } from "node:crypto";
import { z } from "zod";
import { decode, encode, field } from "../exact-json.ts";
import type { CentralOwner } from "./central-owner.ts";
import type { Receipt } from "../receipt.ts";

const copyAddressSchema = z.object({ kind: z.literal("copy"), value: z.string() });
const dispatchSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("idle") }),
  z.object({ kind: z.literal("armed"), value: z.object({ destination: copyAddressSchema }) }),
  z.object({
    kind: z.literal("held"),
    value: z.object({
      destination: copyAddressSchema,
      request: z.array(z.bigint().min(0n).max(255n)),
    }),
  }),
]);
export const positionSchema = z.object({
  epoch: z.bigint().nonnegative(),
  sequence: z.bigint().nonnegative(),
});
const pieceSchema = z.object({
  database: z.string(),
  epoch: z.bigint().nonnegative(),
  continuesFrom: positionSchema,
  through: positionSchema,
  transactions: z.array(z.object({ kind: z.string(), rows: z.array(z.unknown()).optional() })),
  digest: z.string(),
});
const postRowSchema = z.object({
  kind: z.literal("upsert"),
  table: z.literal("posts"),
  key: z.tuple([z.object({ kind: z.literal("text"), value: z.string() })]),
});
type Command =
  | "holdNextDispatch"
  | "awaitHeldDispatch"
  | "resumeDispatch"
  | "resumeDispatchAndHoldNext"
  | "observe";
export class CopyHold {
  private armed = false;
  constructor(
    private readonly input: {
      owner: CentralOwner;
      service: string;
      project: string;
      receipt: Receipt;
    },
  ) {}
  private async command(kind: Command): Promise<z.infer<typeof dispatchSchema>> {
    const destination = { kind: "copy", value: this.input.project };
    const reply = await this.input.owner.control(
      { kind: "connection", value: this.input.service },
      kind === "holdNextDispatch" || kind === "resumeDispatchAndHoldNext"
        ? { kind, value: { destination } }
        : { kind },
    );
    const dispatch = dispatchSchema.parse(field(reply, "dispatch"));
    if (dispatch.kind !== "idle" && dispatch.value.destination.value !== this.input.project)
      throw new Error("control selected another physical Copy");
    await this.input.receipt.record("copyDispatchControl", { command: kind, state: dispatch.kind });
    return dispatch;
  }
  async arm(): Promise<void> {
    this.armed = true;
    const dispatch = await this.command("holdNextDispatch");
    if (dispatch.kind !== "armed") throw new Error("Copy hold did not arm");
  }
  async cause(post: string): Promise<z.infer<typeof pieceSchema>> {
    for (let attempt = 0; attempt < 16; attempt += 1) {
      const dispatch = await this.command("awaitHeldDispatch");
      if (dispatch.kind !== "held") throw new Error("Copy dispatch not held");
      const raw = Buffer.from(dispatch.value.request.map((byte): number => Number(byte)));
      const envelope = decode(raw.toString("utf8"));
      const request = z
        .object({ kind: z.string(), value: z.unknown() })
        .parse(field(envelope, "request", "request"));
      if (request.kind === "piece") {
        const piece = pieceSchema.parse(request.value);
        if (piece.database !== this.input.project)
          throw new Error("held Piece names another project");
        const hasPost = piece.transactions.some(
          (transaction): boolean =>
            transaction.kind === "rows" &&
            (transaction.rows ?? []).some((row): boolean => {
              const parsed = postRowSchema.safeParse(row);
              return parsed.success && parsed.data.key[0].value === post;
            }),
        );
        if (hasPost) {
          await this.input.receipt.record("heldCausePiece", {
            requestSHA256: createHash("sha256").update(raw).digest("hex"),
            pieceSHA256: createHash("sha256").update(encode(piece)).digest("hex"),
            through: piece.through,
            continuesFrom: piece.continuesFrom,
            transactions: piece.transactions.length,
          });
          return piece;
        }
      }
      await this.command("resumeDispatchAndHoldNext");
    }
    throw new Error("held normal Copy Piece never contained the exact cause post row");
  }
  async close(): Promise<void> {
    if (!this.armed) return;
    const observed = await this.command("observe");
    if (observed.kind === "idle") {
      this.armed = false;
      return;
    }
    if (observed.kind === "armed") await this.command("awaitHeldDispatch");
    await this.command("resumeDispatch");
    this.armed = false;
  }
}
