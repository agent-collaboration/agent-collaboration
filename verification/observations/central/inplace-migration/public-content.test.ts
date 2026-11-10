import { test } from "node:test";
import assert from "node:assert/strict";
import { collectPublicContent } from "./public-content.ts";
import type { RouterRead } from "./router-lists.ts";
const project = "01900000-0000-7000-8000-000000000001",
  reader = "01900000-0000-7000-8000-000000000002",
  holder = "01900000-0000-7000-8000-000000000003";
const source = {
  artifact: "/fixture/public-content.json",
  sha256: "a".repeat(64),
  method: "routerPublicRead",
} as const;
function readContent(empty = false, unavailable = false): RouterRead {
  return async (arguments_) => {
    const kind = arguments_[0];
    const facts = {
      [`${kind}Id`]: `01900000-0000-7000-8000-00000000001${kind === "post" ? "1" : kind === "task" ? "2" : "3"}`,
      projectId: project,
      text: "literal independently-read content",
    };
    return {
      ...source,
      body: {
        kind: "result",
        cliVersion: "0.1.125",
        result: {
          page: {
            records: empty ? [] : [kind === "post" ? facts : { kind, record: facts }],
            nextCursor: null,
            unavailable: unavailable ? [project] : [],
          },
        },
      },
    };
  };
}

test("non-holder captures every selected public collection and requires real records", async (): Promise<void> => {
  const result = await collectPublicContent({
    request: { project, readerInstallation: reader, holderInstallation: holder },
    read: readContent(),
    expectedVersion: "0.1.125",
    source,
  });
  assert.equal(result.records.length, 3);
  assert.equal(result.readerInstallation, reader);
});
test("an explicitly unheld project admits a public reader without inventing a holder", async () => {
  const result = await collectPublicContent({
    request: { project, readerInstallation: reader, holderInstallation: null },
    read: readContent(),
    expectedVersion: "0.1.125",
    source,
  });
  assert.equal(result.holderInstallation, null);
  assert.equal(result.records.length, 3);
});
test("holder-only, empty, unavailable and wrong-version views are refused", async (): Promise<void> => {
  for (const input of [
    {
      request: { project, readerInstallation: holder, holderInstallation: holder },
      read: readContent(),
      expectedVersion: "0.1.125",
      source,
    },
    {
      request: { project, readerInstallation: reader, holderInstallation: holder },
      read: readContent(true),
      expectedVersion: "0.1.125",
      source,
    },
    {
      request: { project, readerInstallation: reader, holderInstallation: holder },
      read: readContent(false, true),
      expectedVersion: "0.1.125",
      source,
    },
    {
      request: { project, readerInstallation: reader, holderInstallation: holder },
      read: readContent(),
      expectedVersion: "0.1.135",
      source,
    },
  ])
    await assert.rejects(collectPublicContent(input));
});
