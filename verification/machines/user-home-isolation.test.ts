import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, realpath, rm, writeFile, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Machine } from "../declaration.ts";
import {
  prepareStagingUserHome,
  validateStagingUserHome,
  hashPrivateRegistry,
} from "./user-home-isolation.ts";
test("private child HOME starts empty and registry fingerprint detects external changes", async (): Promise<void> => {
  const temporary = await mkdtemp(join(tmpdir(), "2026-11-private-user-home-")),
    parent = await realpath(temporary);
  const artifact = {
    path: "/fixture/binary",
    sha256: "a".repeat(64),
    identifier: "fixture",
    team: "fixture",
  };
  const machine: Machine = {
    kind: "local",
    name: "sunbook",
    root: `${parent}/router`,
    temporaryParent: parent,
    nativeCwd: parent,
    codexHome: `${parent}/codex-home`,
    port: 18791,
    router: artifact,
    cli: artifact,
    enrollment: { kind: "unattached" },
  };
  const home = `${parent}/home`;
  try {
    await prepareStagingUserHome({ machine, home });
    const registry = `${home}/.claude/sessions`,
      before = await hashPrivateRegistry(registry);
    await validateStagingUserHome({ machine, home });
    await writeFile(`${registry}/synthetic-peer.json`, "synthetic metadata", { mode: 0o600 });
    assert.notEqual(await hashPrivateRegistry(registry), before);
    await assert.rejects(validateStagingUserHome({ machine, home }), /empty private/);
    await rm(`${registry}/synthetic-peer.json`);
    await symlink(`${parent}/outside`, `${registry}/synthetic-link`);
    await assert.rejects(hashPrivateRegistry(registry), /symlink/);
    await assert.rejects(
      prepareStagingUserHome({ machine, home: `${parent}/../outside` }),
      /separate/,
    );
  } finally {
    await rm(temporary, { recursive: true });
  }
});
