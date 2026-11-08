import test from "node:test";
import assert from "node:assert/strict";
import template from "../cloudflare.config.ts";
import linkTemplate from "../cloudflare-link.config.ts";

test("the public deployment template declares the required private delivery configuration", () => {
  assert.deepEqual(template.worker.env.CENTRAL_DELIVERY_CONFIG, {
    type: "text",
    value: "REPLACE_DELIVERY_CONFIG_JSON",
  });
  assert.equal(template.worker.workersDev, false);
  assert.equal(template.worker.previewUrls, false);
});

test("the public template names the sign-in trust that checks organization stamps", () => {
  for (const name of ["CENTRAL_SIGN_IN_ISSUER", "CENTRAL_SIGN_IN_AUDIENCE", "CENTRAL_SIGN_IN_JWKS_URL"])
    assert.match(template.worker.env[name].value, /^REPLACE_/);
});

test("the central-link template has no public surface and borrows central's Fleet class", () => {
  assert.equal(linkTemplate.worker.workersDev, false);
  assert.equal(linkTemplate.worker.previewUrls, false);
  assert.equal(linkTemplate.worker.exports, undefined);
  assert.equal(linkTemplate.worker.routes, undefined);
  assert.deepEqual(linkTemplate.worker.env.FLEET, {
    type: "durable-object",
    worker: "replace-central-worker-name",
    exportName: "FleetObject",
  });
  assert.equal(linkTemplate.worker.env.PROJECT_COPIES, undefined);
});
