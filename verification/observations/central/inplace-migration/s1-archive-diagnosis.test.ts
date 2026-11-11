import assert from "node:assert/strict";
import { test } from "node:test";
import { archiveStage } from "./s1-archive-diagnosis.ts";

test("archive progress requires the authoritative sender ACK to cover the fact's mapped feed cursor", (): void => {
  const facts = [
    {
      envelope: {
        position: { kind: "project", position: { cursor: { epoch: 1n, position: 12n } } },
      },
    },
  ];
  const base = {
    kind: "archiveProgressObserved",
    untouchedBeforeSha256: "sealed",
    untouchedAfterSha256: "sealed",
  };
  const ack = {
    copy: "01a16eaa-ae36-7eec-9698-e8c34d10238b",
    role: "authoritative",
    acknowledgedFeed: { epoch: 1n, position: 11n },
  };
  assert.equal(
    archiveStage({ ...base, archiveFacts: [], acknowledgements: [ack] }),
    "archiveFactAbsent",
  );
  assert.equal(
    archiveStage({ ...base, archiveFacts: facts, acknowledgements: [ack] }),
    "archiveFactUnacknowledged",
  );
  assert.equal(
    archiveStage({
      ...base,
      archiveFacts: facts,
      acknowledgements: [
        { ...ack, role: "follower", acknowledgedFeed: { epoch: 1n, position: 12n } },
      ],
    }),
    "archiveFactUnacknowledged",
  );
  assert.equal(
    archiveStage({
      ...base,
      archiveFacts: facts,
      acknowledgements: [{ ...ack, acknowledgedFeed: { epoch: 1n, position: 12n } }],
    }),
    "archiveFactAcknowledged",
  );
});
