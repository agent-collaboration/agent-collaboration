import { z } from "zod";

const absolutePath = z.string().startsWith("/");
export const fileSchema = z.object({
  path: absolutePath,
  sha256: z.string().regex(/^[a-f0-9]{64}$/),
});
export const sealedCopySchema = fileSchema.extend({ copyRoot: absolutePath });
export const artifactSchema = fileSchema.extend({
  identifier: z.string().min(1),
  team: z.string().min(1),
});
export type Artifact = z.infer<typeof artifactSchema>;
const endpointSchema = z.object({ serviceId: z.uuid(), endpointId: z.string().min(1) });
export const sessionSchema = z.object({ endpoint: endpointSchema, sessionId: z.string().min(1) });
export type Session = z.infer<typeof sessionSchema>;
export const identitySchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("session"), session: sessionSchema }),
  z.object({ kind: z.literal("human"), humanId: z.string().min(1) }),
]);
export type Identity = z.infer<typeof identitySchema>;
export const environmentName = z.string().regex(/^[A-Z][A-Z0-9_]*$/);
export const enrollmentInputSchema = z.object({
  endpoint: z.url(),
  label: z.string().min(1),
  accessClientId: z.string().min(1),
  accessSecretFile: absolutePath,
  expiresAt: z.iso.datetime(),
  ownerTokenEnv: environmentName,
});
export type EnrollmentInput = z.infer<typeof enrollmentInputSchema>;
const machineBase = z.object({
  name: z.string().regex(/^[a-z][a-z0-9-]*$/),
  root: absolutePath,
  temporaryParent: absolutePath,
  nativeCwd: absolutePath,
  codexHome: absolutePath,
  port: z
    .number()
    .int()
    .min(1024)
    .max(65535)
    .refine((value): boolean => ![8787, 8788, 43127].includes(value)),
  router: artifactSchema,
  cli: artifactSchema,
  enrollment: z
    .discriminatedUnion("kind", [
      z.object({ kind: z.literal("unattached") }),
      z.object({ kind: z.literal("fixture") }),
      enrollmentInputSchema.extend({ kind: z.literal("access") }),
    ])
    .default({ kind: "unattached" }),
});
export const machineSchema = z.discriminatedUnion("kind", [
  machineBase.extend({ kind: z.literal("local") }),
  machineBase.extend({
    kind: z.literal("ssh"),
    host: z.string().regex(/^[a-zA-Z0-9_.@:-]+$/),
    retainedRoots: z.array(absolutePath).min(1),
  }),
]);
export type Machine = z.infer<typeof machineSchema>;
const localTarget = z.object({
  assertionFile: absolutePath,
  root: absolutePath,
  fixtureKey: fileSchema,
});
const deploySchema = z.object({
  executable: fileSchema,
  arguments: z.array(z.string()),
  cwd: absolutePath,
  timeoutSeconds: z.number().int().positive().max(900),
  cfBundle: fileSchema,
  undiciFactoryExport: z.string().min(1),
  inventoryArguments: z.array(z.string()),
});
export const targetSchema = z.discriminatedUnion("kind", [
  localTarget.extend({
    kind: z.literal("tokio"),
    binary: artifactSchema,
    projects: z.array(z.uuid()).min(1),
  }),
  localTarget.extend({
    kind: z.literal("workerd"),
    jwksFile: absolutePath,
    bundleDirectory: absolutePath,
    javascriptSHA256: z.string().regex(/^[a-f0-9]{64}$/),
    wasmSHA256: z.string().regex(/^[a-f0-9]{64}$/),
    template: absolutePath,
  }),
  z.object({
    kind: z.literal("staging"),
    origin: z.url().startsWith("https://"),
    ownerTokenEnv: environmentName,
    accessPolicyIdentity: z.string().min(1),
    deploy: deploySchema,
  }),
]);
export type Target = z.infer<typeof targetSchema>;
export const declarationSchema = z
  .object({
    repository: absolutePath,
    productPin: z.string().regex(/^[a-f0-9]{40}$/),
    productVersion: z.string().regex(/^\d+\.\d+\.\d+$/),
    harnessPin: z.string().regex(/^[a-f0-9]{40}$/),
    purpose: z.enum(["discovery", "confirmation"]),
    importer: artifactSchema,
    evidenceDirectory: absolutePath,
    vd: z.object({
      repository: absolutePath,
      revision: z.string().regex(/^[a-f0-9]{8,40}$/),
      path: z.string().regex(/^docs\/specs\/.+\.md$/),
    }),
    rung: z.enum(["R1", "R2", "R3"]),
    rows: z
      .array(z.string().min(1))
      .min(1)
      .refine((rows): boolean => new Set(rows).size === rows.length),
    target: targetSchema,
    machines: z.array(machineSchema).length(2),
    recipient: z.object({
      model: z.literal("gpt-6-luna"),
      provider: z.literal("openai"),
      effort: z.enum(["low", "medium", "high"]),
    }),
    deadline: z.iso.datetime(),
    legacyStores: z.array(sealedCopySchema.extend({ oracle: artifactSchema })).min(1),
  })
  .superRefine((declaration, context): void => {
    const expected = { R1: "tokio", R2: "workerd", R3: "staging" }[declaration.rung];
    if (declaration.target.kind !== expected)
      context.addIssue({
        code: "custom",
        path: ["target"],
        message: "target does not match the rung",
      });
    if (declaration.machines.filter((machine): boolean => machine.kind === "ssh").length !== 1)
      context.addIssue({
        code: "custom",
        path: ["machines"],
        message: "one local and one SSH installation required",
      });
    if (new Set(declaration.machines.map((machine): string => machine.name)).size !== 2)
      context.addIssue({
        code: "custom",
        path: ["machines"],
        message: "distinct installation names required",
      });
    for (const [index, machine] of declaration.machines.entries()) {
      if (
        machine.enrollment.kind !== (declaration.target.kind === "staging" ? "access" : "fixture")
      )
        context.addIssue({
          code: "custom",
          path: ["machines", index, "enrollment"],
          message: "enrollment does not match the target",
        });
    }
  });
export type Declaration = z.infer<typeof declarationSchema>;
