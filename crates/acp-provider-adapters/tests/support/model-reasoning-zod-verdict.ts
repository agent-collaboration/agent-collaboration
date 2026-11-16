import { modelReasoningSchema } from "../../../../providers/claude-acp/src/model-metadata.ts";

// The owning Zod schema's verdict on one raw `_meta.codexRouter.modelReasoning` value (argument 2,
// JSON). Prints `{"accepted": <the value Zod keeps>}` or `{"rejected": <Zod's issues>}`, the shape the
// N12 tap compares with the Rust decoder's outcome.
const raw: unknown = JSON.parse(process.argv[2] ?? "null");
const verdict = modelReasoningSchema.safeParse(raw);
process.stdout.write(
  JSON.stringify(
    verdict.success
      ? { accepted: verdict.data }
      : {
          rejected: verdict.error.issues.map((issue) => ({
            path: issue.path.map(String).join("/"),
            code: issue.code,
          })),
        },
  ),
);
