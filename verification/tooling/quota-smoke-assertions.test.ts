import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";
import { z } from "zod";
import { decodeEvidenceJson } from "../evidence-json.ts";
import { quotaSmokeAssertions } from "./quota-smoke-assertions.ts";

const caseSchema = z.object({
  mode: z.string(),
  secret: z.string(),
  payload: z.record(z.string(), z.unknown()),
  originalSHA256: z.string(),
  cases: z.array(
    z.object({
      name: z.string(),
      path: z.array(z.string()),
      action: z.enum(["keep", "delete", "set"]),
      value: z.unknown().optional(),
      accepted: z.boolean(),
    }),
  ),
});
const directory = new URL("./fixtures/", import.meta.url);
for (const name of readdirSync(directory).filter((name) => /^quota-smoke-.*\.json$/.test(name))) {
  const fixture = caseSchema.parse(decodeEvidenceJson(readFileSync(new URL(name, directory))));
  for (const sample of fixture.cases)
    test(`quota smoke assertion matches original: ${fixture.mode} ${sample.name}`, (): void => {
      const payload = structuredClone(fixture.payload);
      if (sample.action !== "keep") {
        let parent: object = payload;
        for (const part of sample.path.slice(0, -1)) {
          const child: unknown = Reflect.get(parent, part);
          assert.ok(child !== null && typeof child === "object");
          parent = child;
        }
        const final = z.string().parse(sample.path.at(-1));
        if (sample.action === "delete") Reflect.deleteProperty(parent, final);
        else Reflect.set(parent, final, sample.value);
      }
      let accepted = false;
      try {
        accepted = quotaSmokeAssertions(fixture.mode, payload, fixture.secret);
      } catch {
        /* The original assertion program exits nonzero on refusal. */
      }
      assert.equal(accepted, sample.accepted);
    });
}
