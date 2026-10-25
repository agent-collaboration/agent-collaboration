import { z } from "zod";
import type { EnrollmentInput } from "../declaration.ts";
import { decode } from "../exact-json.ts";
import type { Installation } from "../machines/installation.ts";
import { command } from "../machines/owned-process.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";

const enrolledSchema = z.object({
  kind: z.literal("machineEnrolled"),
  value: z.object({
    peer: z.uuid(),
    expiresAt: z.iso.datetime(),
    authoritativeReceiver: z.uuid(),
    relayPeer: z.uuid(),
  }),
});
const refusalSchema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("refused"),
    value: z.enum([
      "peerInUse",
      "peerMismatch",
      "accessClientInUse",
      "credentialHashInUse",
      "invalidInput",
    ]),
  }),
  z.object({ kind: z.literal("enrollmentInProgress") }),
  z.object({ kind: z.literal("revocationInProgress") }),
]);
const decisionSchema = z.union([enrolledSchema, refusalSchema]);
const replySchema = z.object({
  decision: decisionSchema,
  activation: z.literal("nextRouterStart"),
});
export type EnrollmentOutcome =
  | { readonly kind: "published"; readonly enrollment: z.infer<typeof enrolledSchema>["value"] }
  | { readonly kind: "refused"; readonly decision: z.infer<typeof refusalSchema> };
function normalizedExpiry(value: string): string {
  return value.replace(/\.(\d+)Z$/, (_match, digits: string): string => {
    const fraction = digits.replace(/0+$/, "");
    return fraction === "" ? "Z" : `.${fraction}Z`;
  });
}
async function invoke(input: {
  installation: Installation;
  arguments: readonly string[];
  environment: Readonly<Record<string, string>>;
}): Promise<unknown> {
  const remote = input.installation.remote;
  const binary = input.installation.machine.router;
  let result;
  try {
    result =
      remote === undefined
        ? await command(binary.path, input.arguments, {
            environment: { ...process.env, ...input.environment },
            timeoutSeconds: 60,
          })
        : await remote.output([remote.pathFor(binary), ...input.arguments], {
            environment: input.environment,
            timeoutSeconds: 60,
          });
  } catch {
    throw new ObservationOpen(
      "fleet producer transport unconfirmed; never automatically replay enrollment",
    );
  }
  if (result.exit !== 0)
    throw new ObservationOpen(
      "fleet producer failed; central effect and local publication require reconciliation",
    );
  try {
    return decode(result.stdout);
  } catch {
    throw new ObservationOpen("fleet producer reply invalid; publication unconfirmed");
  }
}
export async function enrollInstallation(input: {
  installation: Installation;
  enrollment: EnrollmentInput;
  receipt: Receipt;
  ownerToken: string;
}): Promise<EnrollmentOutcome> {
  const { installation, enrollment } = input;
  if (Date.parse(enrollment.expiresAt) <= Date.now())
    throw new Error("enrollment expiration has passed");
  const reply = replySchema.parse(
    await invoke({
      installation,
      arguments: [
        "fleet",
        "enroll",
        "--router-root",
        installation.machine.root,
        "--endpoint",
        enrollment.endpoint,
        "--label",
        enrollment.label,
        "--access-client-id",
        enrollment.accessClientId,
        "--access-client-secret-file",
        enrollment.accessSecretFile,
        "--expires-at",
        enrollment.expiresAt,
        "--owner-token-env",
        enrollment.ownerTokenEnv,
        "--allow-plaintext-file-secrets",
      ],
      environment: { [enrollment.ownerTokenEnv]: input.ownerToken },
    }),
  );
  if (reply.decision.kind !== "machineEnrolled")
    return { kind: "refused", decision: reply.decision };
  const status = z
    .object({
      activation: z.literal("nextRouterStart"),
      status: z.object({
        kind: z.literal("valid"),
        enrollment: z.object({
          peer: z.uuid(),
          endpoint: z.url(),
          expiresAt: z.iso.datetime(),
        }),
      }),
    })
    .parse(
      await invoke({
        installation,
        arguments: ["fleet", "status", "--router-root", installation.machine.root],
        environment: {},
      }),
    );
  await input.receipt.equal(
    "product-published metadata matches authoritative enrollment",
    {
      ...status.status.enrollment,
      expiresAt: normalizedExpiry(status.status.enrollment.expiresAt),
    },
    {
      peer: reply.decision.value.peer,
      endpoint: new URL(enrollment.endpoint).href,
      expiresAt: normalizedExpiry(enrollment.expiresAt),
    },
  );
  await input.receipt.equal(
    "central expiry preserves the declared Access expiry",
    normalizedExpiry(reply.decision.value.expiresAt),
    normalizedExpiry(enrollment.expiresAt),
  );
  await input.receipt.record("fleetEnrollmentPublished", {
    peer: reply.decision.value.peer,
    expiresAt: reply.decision.value.expiresAt,
    activation: reply.activation,
    attachmentObserved: false,
  });
  return { kind: "published", enrollment: reply.decision.value };
}
