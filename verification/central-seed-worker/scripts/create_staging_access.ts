// Creates the S1 staging central's Access pieces with the cf CLI, following
// fleet-ops' C3 rehearsal pattern: two debug-Router service tokens, the
// whole-host owner application and the more specific /v1/session application
// (Service Auth for the two tokens). Run from an owner-private scratch
// directory on the owner's cf login.
//
//   node scripts/create_staging_access.ts <settings.json> <secrets dir> <step> [--dry-run]
//     step: tokens | apps | readback
//
// - tokens: refuses a token name that already exists. Each real response, the
//   only copy of its client secret, goes once into <secrets dir>/<name>.json
//   (directory 0700, file 0600). Only the name and id enter the settings.
// - apps: refuses an application on the hostname that already exists. It
//   writes the returned audiences and ids into the settings.
// - readback: prints names, ids and policies only; never a secret.
// Every cf call runs with CLOUDFLARE_ACCOUNT_ID from the settings in its own
// environment; nothing secret is printed or put in an argument.

import { execFile } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";
import {
  PRODUCTION,
  canonicalName,
} from "../../../tools/central-migration/scripts/worker_deploy.ts";

const run = promisify(execFile);
const [settingsPath, secretsDir, step, ...flags] = process.argv.slice(2);
if (
  settingsPath === undefined ||
  secretsDir === undefined ||
  !["tokens", "apps", "readback"].includes(step ?? "")
) {
  throw new Error(
    "usage: create_staging_access.ts <settings.json> <secrets dir> <tokens|apps|readback> [--dry-run]",
  );
}
const settingsFile: string = settingsPath;
const secrets: string = secretsDir;
const dryRun = flags.includes("--dry-run");
const TOKEN_NAMES = ["s1-sunbook-debug-router", "s1-sunclaw-debug-router"] as const;
const TOKEN_DURATION = "720h";

interface S1Settings {
  accountId: string;
  workerName: string;
  hostname: string;
  access: Record<string, unknown> & { ownerIdentity: string };
}
const settings = JSON.parse(await readFile(settingsFile, "utf8")) as S1Settings;
if (
  canonicalName(settings.hostname) === PRODUCTION.domain ||
  canonicalName(settings.workerName) === PRODUCTION.worker
) {
  throw new Error("refusing production's central");
}

async function cf(args: readonly string[]): Promise<unknown> {
  const { stdout } = await run("cf", [...args], {
    env: { ...process.env, CLOUDFLARE_ACCOUNT_ID: settings.accountId },
    maxBuffer: 16 * 1024 * 1024,
  });
  const text = stdout.trim();
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return { text };
  }
}
const resultOf = (answer: unknown): unknown =>
  (answer as { result?: unknown } | null)?.result ?? answer;
const listOf = (answer: unknown): Record<string, unknown>[] => {
  const result = resultOf(answer);
  return Array.isArray(result) ? (result as Record<string, unknown>[]) : [];
};
const saveSettings = () =>
  writeFile(settingsFile, `${JSON.stringify(settings, null, 2)}\n`, { mode: 0o600 });

async function tokens() {
  const existing = listOf(await cf(["zero-trust", "access", "service-tokens", "list"]));
  const recorded: { name: string; id: string; expiresAt: unknown }[] = [];
  for (const name of TOKEN_NAMES) {
    if (existing.some((token) => token.name === name))
      throw new Error(`service token ${name} already exists; reconcile it, don't create another`);
    const args = [
      "zero-trust",
      "access",
      "service-tokens",
      "create",
      "--name",
      name,
      "--duration",
      TOKEN_DURATION,
    ];
    if (dryRun) {
      const answer = await cf([...args, "--dry-run"]);
      console.log(
        JSON.stringify({
          step: "tokens",
          name,
          dryRun: true,
          answerKeys: Object.keys(answer as object),
        }),
      );
      continue;
    }
    const { stdout } = await run("cf", args, {
      env: { ...process.env, CLOUDFLARE_ACCOUNT_ID: settings.accountId },
    });
    await mkdir(secrets, { recursive: true, mode: 0o700 });
    await writeFile(join(secrets, `${name}.json`), stdout, { mode: 0o600, flag: "wx" });
    const token = resultOf(JSON.parse(stdout)) as { id?: string; expires_at?: unknown };
    if (typeof token.id !== "string")
      throw new Error(`token ${name}: the response has no id; its raw answer is in ${secrets}`);
    recorded.push({ name, id: token.id, expiresAt: token.expires_at });
  }
  if (!dryRun) {
    settings.access.serviceTokens = recorded;
    await saveSettings();
    console.log(
      JSON.stringify({ step: "tokens", created: recorded.map(({ name, id }) => ({ name, id })) }),
    );
  }
}

async function apps() {
  const tokens = settings.access.serviceTokens as { name: string; id: string }[] | undefined;
  if (!Array.isArray(tokens) || tokens.length !== TOKEN_NAMES.length)
    throw new Error("create the service tokens first");
  const existing = listOf(
    await cf(["zero-trust", "access", "applications", "list", "--domain", settings.hostname]),
  );
  if (existing.length > 0)
    throw new Error(`an Access application for ${settings.hostname} already exists; reconcile it`);
  const bodies = {
    admin: {
      type: "self_hosted",
      name: `${settings.workerName} owner`,
      domain: settings.hostname,
      session_duration: "24h",
      policies: [
        {
          name: "owner",
          decision: "allow",
          include: [{ email: { email: settings.access.ownerIdentity } }],
        },
      ],
    },
    session: {
      type: "self_hosted",
      name: `${settings.workerName} sessions`,
      domain: `${settings.hostname}/v1/session`,
      session_duration: "24h",
      policies: [
        {
          name: "debug routers",
          decision: "non_identity",
          include: tokens.map((token) => ({ service_token: { token_id: token.id } })),
        },
      ],
    },
  };
  const created: Record<string, { id: unknown; aud: unknown }> = {};
  for (const [kind, body] of Object.entries(bodies)) {
    const answer = await cf([
      "zero-trust",
      "access",
      "applications",
      "create",
      "--body",
      JSON.stringify(body),
      ...(dryRun ? ["--dry-run"] : []),
    ]);
    if (dryRun) {
      console.log(
        JSON.stringify({
          step: "apps",
          kind,
          dryRun: true,
          domain: body.domain,
          policies: body.policies.map((policy) => ({
            name: policy.name,
            decision: policy.decision,
            include: policy.include.map((rule) => Object.keys(rule)[0]),
          })),
        }),
      );
      continue;
    }
    const app = resultOf(answer) as { id?: unknown; aud?: unknown };
    if (typeof app.aud !== "string")
      throw new Error(`${kind} application: the response has no audience`);
    created[kind] = { id: app.id, aud: app.aud };
  }
  if (!dryRun) {
    settings.access.adminAudience = created.admin?.aud;
    settings.access.sessionAudience = created.session?.aud;
    settings.access.applications = { admin: created.admin?.id, session: created.session?.id };
    await saveSettings();
    console.log(
      JSON.stringify({
        step: "apps",
        created: Object.fromEntries(
          Object.entries(created).map(([kind, app]) => [kind, { id: app.id }]),
        ),
      }),
    );
  }
}

async function readback() {
  const apps = listOf(
    await cf(["zero-trust", "access", "applications", "list", "--domain", settings.hostname]),
  );
  const allTokens = listOf(await cf(["zero-trust", "access", "service-tokens", "list"]));
  const tokens = allTokens.filter((token) =>
    (TOKEN_NAMES as readonly string[]).includes(String(token.name)),
  );
  console.log(
    JSON.stringify(
      {
        applications: apps.map((app) => ({
          name: app.name,
          id: app.id,
          policies: ((app.policies as Record<string, unknown>[] | undefined) ?? []).map(
            (policy) => ({
              name: policy.name,
              decision: policy.decision,
              include: policy.include,
            }),
          ),
        })),
        serviceTokens: tokens.map((token) => ({
          name: token.name,
          id: token.id,
          expiresAt: token.expires_at,
        })),
      },
      null,
      2,
    ),
  );
}

if (step === "tokens") await tokens();
else if (step === "apps") await apps();
else await readback();
