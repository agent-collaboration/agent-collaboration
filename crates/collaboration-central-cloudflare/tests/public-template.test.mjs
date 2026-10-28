import test from "node:test";
import assert from "node:assert/strict";
import template from "../cloudflare.config.ts";

test("the public deployment template declares the required private delivery configuration", () => {
  assert.deepEqual(template.worker.env.CENTRAL_DELIVERY_CONFIG, {
    type: "text",
    value: "REPLACE_DELIVERY_CONFIG_JSON",
  });
  assert.equal(template.worker.workersDev, false);
  assert.equal(template.worker.previewUrls, false);
});
