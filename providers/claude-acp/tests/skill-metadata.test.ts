import assert from "node:assert/strict";
import { test } from "node:test";
import { SkillMetadataDecorator } from "../src/skill-metadata.ts";
import type { MetadataNotification, MetadataQuery } from "../src/skill-metadata.ts";

const descriptor = { name: "review", description: "Review", argumentHint: "<file>" };
function notification(names: string[] = ["review", "compact"]): MetadataNotification {
  return {
    sessionId: "session",
    update: {
      sessionUpdate: "available_commands_update",
      availableCommands: names.map((name) => ({
        name,
        description: name,
        _meta: { original: true },
      })),
    },
  };
}
function query(): MetadataQuery {
  return {
    sessionIdentity: {},
    queryIdentity: {},
    supportedCommands: async (): Promise<unknown> => [
      descriptor,
      { name: "compact", description: "Compact", argumentHint: "", builtin: true },
    ],
    reloadSkills: async (): Promise<unknown> => ({ skills: [descriptor] }),
  };
}

await test("forwards original immediately, annotates only the uniquely joined existing skill", async (): Promise<void> => {
  const sent: MetadataNotification[] = [];
  const current = query();
  const decorator = new SkillMetadataDecorator({
    currentQuery: (): MetadataQuery => current,
    send: async (message): Promise<void> => {
      sent.push(message);
    },
  });
  const original = notification();
  await decorator.sessionUpdate(original);
  assert.equal(sent[0], original);
  await decorator.settled();
  assert.deepEqual(sent.at(-1)?.update, {
    sessionUpdate: "available_commands_update",
    availableCommands: [
      {
        name: "review",
        description: "review",
        _meta: { original: true, codexRouter: { kind: "skill" } },
      },
      { name: "compact", description: "compact", _meta: { original: true } },
    ],
    _meta: { codexRouter: { skillStatus: "available" } },
  });
});

await test("coalesces reload echoes and cached failures without a second reload", async (): Promise<void> => {
  const sent: MetadataNotification[] = [];
  let reads = 0;
  let decorator: SkillMetadataDecorator;
  const current = {
    ...query(),
    reloadSkills: async (): Promise<unknown> => {
      reads += 1;
      await decorator.sessionUpdate(notification());
      throw new Error("source failed");
    },
  };
  decorator = new SkillMetadataDecorator({
    currentQuery: (): MetadataQuery => current,
    send: async (message): Promise<void> => {
      sent.push(message);
    },
  });
  await decorator.sessionUpdate(notification());
  await decorator.settled();
  await decorator.sessionUpdate(notification());
  await decorator.settled();
  assert.equal(reads, 1);
  assert.deepEqual(sent.at(-1)?.update._meta, { codexRouter: { skillStatus: "unavailable" } });
});

await test("descriptor-only builtin changes and changed reload result remove old membership", async (): Promise<void> => {
  const sent: MetadataNotification[] = [];
  let builtin = false;
  let reads = 0;
  const current = {
    ...query(),
    supportedCommands: async (): Promise<unknown> => [
      descriptor,
      ...(builtin ? [{ ...descriptor, builtin: true }] : []),
    ],
    reloadSkills: async (): Promise<unknown> => {
      reads += 1;
      return { skills: builtin ? [] : [descriptor] };
    },
  };
  const decorator = new SkillMetadataDecorator({
    currentQuery: (): MetadataQuery => current,
    send: async (message): Promise<void> => {
      sent.push(message);
    },
  });
  await decorator.sessionUpdate(notification());
  await decorator.settled();
  assert.deepEqual(sent.at(-1)?.update.availableCommands?.[0]?._meta?.codexRouter, {
    kind: "skill",
  });
  builtin = true;
  await decorator.sessionUpdate(notification());
  await decorator.settled();
  assert.equal(reads, 2);
  assert.equal(sent.at(-1)?.update.availableCommands?.[0]?._meta?.codexRouter, undefined);
});

await test("retired session state cannot publish even when its query is reused", async (): Promise<void> => {
  const sent: MetadataNotification[] = [];
  const started = Promise.withResolvers<void>();
  const held = Promise.withResolvers<unknown>();
  const identity = {};
  let current: MetadataQuery = {
    ...query(),
    queryIdentity: identity,
    reloadSkills: (): Promise<unknown> => {
      started.resolve();
      return held.promise;
    },
  };
  const decorator = new SkillMetadataDecorator({
    currentQuery: (): MetadataQuery => current,
    send: async (message): Promise<void> => {
      sent.push(message);
    },
  });
  await decorator.sessionUpdate(notification());
  await started.promise;
  current = {
    ...query(),
    queryIdentity: identity,
    reloadSkills: async (): Promise<unknown> => ({ skills: [] }),
  };
  await decorator.sessionUpdate(notification());
  await decorator.settled();
  const count = sent.length;
  held.resolve({ skills: [descriptor] });
  await held.promise;
  await Promise.resolve();
  assert.equal(sent.length, count);
  assert.equal(sent.at(-1)?.update.availableCommands?.[0]?._meta?.codexRouter, undefined);
});

await test("failed descriptor input is cached until its advertised input changes", async (): Promise<void> => {
  let reads = 0;
  const current = {
    ...query(),
    supportedCommands: async (): Promise<unknown> => {
      reads += 1;
      throw new Error("query failed");
    },
  };
  const decorator = new SkillMetadataDecorator({
    currentQuery: (): MetadataQuery => current,
    send: async (): Promise<void> => {},
  });
  await decorator.sessionUpdate(notification());
  await decorator.settled();
  await decorator.sessionUpdate(notification());
  await decorator.settled();
  assert.equal(reads, 1);
  await decorator.sessionUpdate(notification(["other"]));
  await decorator.settled();
  assert.equal(reads, 2);
});

await test("an update arriving between refresh completion and finalization is still derived", async (): Promise<void> => {
  const sent: MetadataNotification[] = [];
  const publishing = Promise.withResolvers<void>();
  const release = Promise.withResolvers<void>();
  let updated = false;
  let followup: Promise<void> | undefined;
  let decorator: SkillMetadataDecorator;
  const current = {
    ...query(),
    supportedCommands: async (): Promise<unknown> => [
      { ...descriptor, name: updated ? "next" : "review" },
    ],
    reloadSkills: async (): Promise<unknown> => ({
      skills: [{ ...descriptor, name: updated ? "next" : "review" }],
    }),
  };
  decorator = new SkillMetadataDecorator({
    currentQuery: (): MetadataQuery => current,
    send: (message): Promise<void> => {
      sent.push(message);
      if (message.update._meta?.codexRouter !== undefined && !updated) {
        followup = release.promise.then(async (): Promise<void> => {
          updated = true;
          await decorator.sessionUpdate(notification(["next"]));
        });
        publishing.resolve();
        return release.promise;
      }
      return Promise.resolve();
    },
  });
  await decorator.sessionUpdate(notification(["review"]));
  await publishing.promise;
  release.resolve();
  await followup;
  await decorator.settled();
  assert.deepEqual(sent.at(-1)?.update.availableCommands?.[0]?._meta?.codexRouter, {
    kind: "skill",
  });
  assert.equal(sent.at(-1)?.update.availableCommands?.[0]?.name, "next");
});

await test("a changed namespace invalidates a prior descriptor failure even if earlier rows return", async (): Promise<void> => {
  let reads = 0;
  const current = {
    ...query(),
    supportedCommands: async (): Promise<unknown> => {
      reads += 1;
      if (reads === 1) throw new Error("first source read failed");
      return [descriptor];
    },
  };
  const decorator = new SkillMetadataDecorator({
    currentQuery: (): MetadataQuery => current,
    send: async (): Promise<void> => {},
  });
  await decorator.sessionUpdate(notification());
  await decorator.settled();
  await decorator.sessionUpdate(notification(["changed"]));
  await decorator.settled();
  await decorator.sessionUpdate(notification());
  await decorator.settled();
  assert.equal(reads, 3);
});

await test("changed advertised rows permit one fresh reload even when SDK descriptors stay identical", async (): Promise<void> => {
  let reloads = 0;
  const current = {
    ...query(),
    reloadSkills: async (): Promise<unknown> => {
      reloads += 1;
      return { skills: [descriptor] };
    },
  };
  const decorator = new SkillMetadataDecorator({
    currentQuery: (): MetadataQuery => current,
    send: async (): Promise<void> => {},
  });
  await decorator.sessionUpdate(notification(["review"]));
  await decorator.settled();
  await decorator.sessionUpdate(notification(["review", "newly-advertised"]));
  await decorator.settled();
  await decorator.sessionUpdate(notification(["review", "newly-advertised"]));
  await decorator.settled();
  assert.equal(reloads, 2);
});
