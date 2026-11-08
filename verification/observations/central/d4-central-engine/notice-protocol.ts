/** Views of the actual single-frame notice protocol; callers retain original bytes. */
import { z } from "zod";
import { decode, encode } from "../../../exact-json.ts";
import { unsigned64Schema } from "./event-observation.ts";

const capabilitySchema = z.object({ batches: z.boolean(), noticesApplied: z.boolean() }).strict();
const welcomeSchema = z
  .object({
    kind: z.literal("welcome"),
    value: z
      .object({
        sessionId: z.uuidv7(),
        databases: z.array(z.unknown()),
        capabilities: capabilitySchema.optional(),
      })
      .strict(),
  })
  .strict();
export type NoticeWelcome = z.infer<typeof welcomeSchema>["value"];

const noticeFrameSchema = z
  .object({
    kind: z.literal("notice"),
    value: z
      .object({
        position: unsigned64Schema,
        body: z
          .object({
            kind: z.literal("consumer"),
            value: z
              .object({
                position: unsigned64Schema,
                kind: z
                  .unknown()
                  .refine((value): boolean => value !== undefined, "notice payload is missing"),
              })
              .strict(),
          })
          .strict(),
      })
      .strict(),
  })
  .strict()
  .transform((frame) => ({
    transportPosition: frame.value.position,
    noticePosition: frame.value.body.value.position,
    notice: frame.value.body.value.kind,
  }));
export type MachineNoticeFrame = z.infer<typeof noticeFrameSchema>;

const acknowledgmentSchema = z
  .object({
    kind: z.literal("answer"),
    value: z
      .object({
        frame: z.uuidv7(),
        body: z
          .object({
            kind: z.literal("noticesApplied"),
            value: z.object({ through: unsigned64Schema }).strict(),
          })
          .strict(),
      })
      .strict(),
  })
  .strict();

export function readNoticeWelcome(payload: Buffer): NoticeWelcome {
  return welcomeSchema.parse(decode(payload.toString("utf8"))).value;
}

export function readMachineNotice(payload: Buffer): MachineNoticeFrame {
  return noticeFrameSchema.parse(decode(payload.toString("utf8")));
}

export function noticeHello(input: {
  readonly session: string;
  readonly upgraded: boolean;
}): Buffer {
  return Buffer.from(
    encode({
      kind: "hello",
      value: {
        sessionId: z.uuidv7().parse(input.session),
        schemaSupport: { maxVersion: 1n },
        ...(input.upgraded ? { capabilities: { batches: false, noticesApplied: true } } : {}),
      },
    }),
  );
}

export function noticesAppliedRequest(input: {
  readonly frame: string;
  readonly through: bigint;
}): Buffer {
  return Buffer.from(
    encode({
      kind: "request",
      value: {
        frame: z.uuidv7().parse(input.frame),
        body: { kind: "noticesApplied", value: { through: unsigned64Schema.parse(input.through) } },
      },
    }),
  );
}

export function readNoticesAppliedAnswer(input: {
  readonly payload: Buffer;
  readonly frame: string;
}): bigint {
  const answer = acknowledgmentSchema.parse(decode(input.payload.toString("utf8")));
  if (answer.value.frame !== z.uuidv7().parse(input.frame))
    throw new Error("NoticesApplied answer names another request frame");
  return answer.value.body.value.through;
}
