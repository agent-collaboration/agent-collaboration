import { test } from "node:test";
import assert from "node:assert/strict";
import { collectPublicContent } from "./public-content.ts";
import { routerObservation } from "./contracts.ts";
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

test("non-holder captures the records from every selected public collection", async (): Promise<void> => {
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
test("three successful terminal empty collections produce a complete empty observation", async (): Promise<void> => {
  const collections: string[] = [];
  const read = readContent(true);
  const result = await collectPublicContent({
    request: { project, readerInstallation: reader, holderInstallation: holder },
    read: async (arguments_) => {
      collections.push(arguments_[0] ?? "");
      return read(arguments_);
    },
    expectedVersion: "0.1.125",
    source,
  });
  assert.deepEqual(collections, ["workstream", "task", "post"]);
  assert.deepEqual(result, {
    project,
    readerInstallation: reader,
    holderInstallation: holder,
    complete: true,
    records: [],
    source,
  });
  assert.deepEqual(routerObservation.shape.content.element.parse(result), result);
});

const emptyPage = { records: [], nextCursor: null, unavailable: [] };
const emptyReply = {
  kind: "result",
  cliVersion: "0.1.125",
  result: { page: emptyPage },
};
for (const [name, body] of [
  ["unavailable", { ...emptyReply, result: { page: { ...emptyPage, unavailable: [project] } } }],
  ["error", { kind: "error", message: "unavailable" }],
  ["unknown", { ...emptyReply, kind: "unknown" }],
  ["wrong CLI version", { ...emptyReply, cliVersion: "0.1.135" }],
  ["wrong service version", { ...emptyReply, serviceVersion: "0.1.135" }],
  ["nonterminal cursor", { ...emptyReply, result: { page: { ...emptyPage, nextCursor: "next" } } }],
  ["invalid cursor", { ...emptyReply, result: { page: { ...emptyPage, nextCursor: 1 } } }],
] as const) {
  test(`an empty ${name} page cannot complete the public observation`, async (): Promise<void> => {
    const collections: string[] = [];
    const read = readContent(true);
    await assert.rejects(
      collectPublicContent({
        request: { project, readerInstallation: reader, holderInstallation: holder },
        read: async (arguments_) => {
          collections.push(arguments_[0] ?? "");
          return arguments_[0] === "post" ? { ...source, body } : read(arguments_);
        },
        expectedVersion: "0.1.125",
        source,
      }),
    );
    assert.deepEqual(collections, ["workstream", "task", "post"]);
  });
}

test("holder-only, unavailable and wrong-version views are refused", async (): Promise<void> => {
  for (const input of [
    {
      request: { project, readerInstallation: holder, holderInstallation: holder },
      read: readContent(),
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
