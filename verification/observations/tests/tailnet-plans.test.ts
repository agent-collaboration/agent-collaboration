import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { z } from "zod";
import { decodeEvidenceJson } from "../../evidence-json.ts";
import { httpsAuthority, httpsEndpointSchema, httpsOrigin } from "../tailnet-plans.ts";

const cases = z
  .array(
    z.object({
      url: z.string(),
      expected: z.union([
        z.object({ authority: z.string(), origin: z.string() }),
        z.object({ refused: z.literal(true) }),
      ]),
    }),
  )
  .parse(decodeEvidenceJson(readFileSync(new URL("baselines/tailnet-urls.json", import.meta.url))));
for (const entry of cases)
  test(`tailnet HTTPS plan matches original URL qualification: ${entry.url}`, (): void => {
    if ("refused" in entry.expected)
      assert.throws(() => httpsEndpointSchema.parse({ url: entry.url }));
    else {
      const target = httpsEndpointSchema.parse({ url: entry.url });
      assert.deepEqual(
        { authority: httpsAuthority(target), origin: httpsOrigin(target) },
        entry.expected,
      );
    }
  });
