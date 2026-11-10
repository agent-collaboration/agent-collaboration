/** The original root/enrollment/installation-to-peer contract, shared by owned upgrade and continuation. */
import { z } from "zod";

export const identitySchema = z.object({
  serviceId: z.uuid(),
  machineLabel: z.string(),
  peerId: z.uuidv7(),
});
export type InstallationIdentity = z.infer<typeof identitySchema>;
export const upgradeBaselineSchema = z
  .object({
    root: z.string(),
    installationId: z.uuid(),
    machineId: z.uuidv7(),
    endpoint: z.url(),
    enrollmentSha256: z.string().regex(/^[a-f0-9]{64}$/),
    baselineId: z.uuidv7(),
  })
  .strict();
export type UpgradeBaseline = z.infer<typeof upgradeBaselineSchema>;
export const enrollmentStatus = z.object({
  status: z.object({
    kind: z.literal("valid"),
    enrollment: z.object({ peer: z.uuidv7(), endpoint: z.url() }),
  }),
});

export function validateUpgradeEnrollment(input: {
  expected: UpgradeBaseline;
  identity: InstallationIdentity;
  status: unknown;
  enrollmentSha256: string;
}): void {
  const observed = enrollmentStatus.parse(input.status).status.enrollment;
  if (
    input.identity.serviceId !== input.expected.installationId ||
    input.identity.peerId !== input.expected.machineId ||
    observed.peer !== input.expected.machineId ||
    new URL(observed.endpoint).href !== new URL(input.expected.endpoint).href ||
    input.enrollmentSha256 !== input.expected.enrollmentSha256
  )
    throw new Error("existing root identity or enrollment differs from the recorded baseline");
}
