import { z } from "zod";
import {
  artifactSchema,
  localMachineSchema,
  runSourceSchema,
  sealedCopySchema,
  tokioTargetSchema,
} from "./declaration.ts";

export const importDeclarationSchema = runSourceSchema
  .extend({
    dataBoundary: z.literal("realCopiesMacOnly"),
    machine: localMachineSchema.refine((machine): boolean => machine.enrollment.kind === "fixture"),
    target: tokioTargetSchema,
    legacyStores: z.array(sealedCopySchema.extend({ oracle: artifactSchema })).min(1),
  })
  .strict();
export type ImportDeclaration = z.infer<typeof importDeclarationSchema>;
