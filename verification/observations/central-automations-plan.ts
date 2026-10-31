/** Declared inputs for the local two-Router automation apparatus, never a product result. */
import { isAbsolute, relative, resolve } from "node:path";
import { z } from "zod";
import { artifactSchema } from "../declaration.ts";

const canonicalPath = z
  .string()
  .refine(
    (value): boolean => isAbsolute(value) && resolve(value) === value,
    "absolute path without traversal required",
  );
export const centralAutomationsPlanSchema = z
  .object({
    repository: canonicalPath,
    sourceHead: z.string().regex(/^[a-f0-9]{40}$/),
    executable: artifactSchema,
    directory: canonicalPath,
    evidence: canonicalPath,
    model: z.literal("claude-haiku-5-5"),
    maxBudgetUsd: z.number().positive().max(0.25),
    purpose: z.literal("bootstrap-only"),
  })
  .strict()
  .superRefine((plan, context): void => {
    for (const field of ["directory", "evidence"]) {
      const value = field === "directory" ? plan.directory : plan.evidence;
      const child = relative(`${plan.repository}/tmp`, value);
      if (child === "" || child === ".." || child.startsWith("../") || isAbsolute(child)) {
        context.addIssue({
          code: "custom",
          path: [field],
          message: "owned directory inside this checkout's tmp required",
        });
      }
    }
    if (Buffer.byteLength(plan.directory) > 78) {
      context.addIssue({
        code: "custom",
        path: ["directory"],
        message: "runtime root too long for macOS Host sockets",
      });
    }
    const artifact = relative(`${plan.repository}/target/debug/examples`, plan.executable.path);
    if (artifact !== "central-automations-rig") {
      context.addIssue({
        code: "custom",
        path: ["executable"],
        message: "use this worktree's normal compiled rig example",
      });
    }
    const evidenceWithinRuntime = relative(plan.directory, plan.evidence);
    const runtimeWithinEvidence = relative(plan.evidence, plan.directory);
    if (
      evidenceWithinRuntime === "" ||
      (evidenceWithinRuntime !== ".." &&
        !evidenceWithinRuntime.startsWith("../") &&
        !isAbsolute(evidenceWithinRuntime)) ||
      (runtimeWithinEvidence !== ".." &&
        !runtimeWithinEvidence.startsWith("../") &&
        !isAbsolute(runtimeWithinEvidence))
    ) {
      context.addIssue({
        code: "custom",
        path: ["evidence"],
        message: "evidence and runtime directories must not overlap",
      });
    }
  });
export type CentralAutomationsPlan = z.infer<typeof centralAutomationsPlanSchema>;
