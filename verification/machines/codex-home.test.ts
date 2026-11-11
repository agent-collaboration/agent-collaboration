import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, realpath, writeFile, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Machine } from "../declaration.ts";
import { CodexHomeGuard, validateStagingCodexHome } from "./codex-home.ts";

test("staging home accepts only minimal private configuration and refuses copied auth/state/session inputs", async (): Promise<void> => {
  const temporary = await mkdtemp(join(tmpdir(), "2026-11-codex-isolation-"));
  const parent = await realpath(temporary),
    home = `${parent}/codex-home`;
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
    codexHome: home,
    port: 18791,
    router: artifact,
    cli: artifact,
    enrollment: { kind: "unattached" },
  };
  try {
    await mkdir(home, { mode: 0o700 });
    const trust = `[projects.${JSON.stringify(parent)}]\ntrust_level = "trusted"\n`;
    await writeFile(`${home}/config.toml`, trust, { mode: 0o600 });
    await writeFile(
      `${home}/codex-router-debug.config.toml`,
      `model_provider = "codex-router-debug"\n${trust}\n[model_providers.codex-router-debug]\nname = "OpenAI"\nbase_url = "http://127.0.0.1:18791/v1"\nwire_api = "responses"\nrequires_openai_auth = false\nsupports_websockets = true\n\n[features]\nenable_request_compression = false\n`,
      { mode: 0o600 },
    );
    await validateStagingCodexHome(machine);
    await assert.rejects(CodexHomeGuard.capture(machine), /normal Codex home required/);
    for (const name of ["auth.json", "state_5.sqlite", "sessions"]) {
      await writeFile(`${home}/${name}`, "synthetic forbidden marker", { mode: 0o600 });
      await assert.rejects(validateStagingCodexHome(machine), /state, sessions, auth/);
      await rm(`${home}/${name}`);
    }
    await rm(`${home}/config.toml`);
    await writeFile(`${parent}/synthetic-config.toml`, trust, { mode: 0o600 });
    await symlink(`${parent}/synthetic-config.toml`, `${home}/config.toml`);
    await assert.rejects(validateStagingCodexHome(machine), /fresh minimal/);
    await assert.rejects(
      validateStagingCodexHome({ ...machine, codexHome: `${parent}/../outside` }),
      /staging-only/,
    );
  } finally {
    await rm(temporary, { recursive: true });
  }
});
