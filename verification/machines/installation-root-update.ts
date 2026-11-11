/** Invoke the existing root updater after owned children settle; never migrate data in this driver. */
import { lstat, readdir, readFile } from "node:fs/promises";
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";
import type { Artifact } from "../declaration.ts";
import type { Receipt } from "../receipt.ts";
import { decodeAuthoredJson } from "../exact-json.ts";
import { registerCommandRoot } from "./command-ledger.ts";
import { command, fingerprint, qualifyArtifact } from "./owned-process.ts";

const disabledProviders = z.object({
  version: z.literal(1),
  providers: z
    .object({
      claude: z.object({ enabled: z.literal(false) }),
      cursor: z.object({ enabled: z.literal(false) }),
    })
    .strict(),
});

const carriedSettingsHeader =
  "// Router settings, carried unchanged from providers.json and ports.json by\n" +
  "// agent-router-update. Edit by hand; the Host reads it at each start.\n";

/** Accept only this fixture's updater-generated settings, not arbitrary JSONC. */
export function requireCarriedDisabledProviders(text: string): void {
  if (!text.startsWith(carriedSettingsHeader))
    throw new Error("updater-generated carried settings required");
  z.object({ providers: disabledProviders })
    .strict()
    .parse(decodeAuthoredJson(text.slice(carriedSettingsHeader.length)));
}

export async function requireDisabledUpdateConfiguration(root: string): Promise<void> {
  try {
    disabledProviders.parse(decodeAuthoredJson(await readFile(`${root}/providers.json`, "utf8")));
  } catch (error: unknown) {
    if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
    requireCarriedDisabledProviders(await readFile(`${root}/settings.jsonc`, "utf8"));
  }
}

export async function qualifyRootUpdater(input: {
  root: string;
  updater: Artifact;
  targetVersion: string;
  receipt: Receipt;
}): Promise<void> {
  if (
    input.updater.identifier !== "dev.shravansunder.agent-router-update.debug" ||
    input.updater.team !== "974QD84WVC"
  )
    throw new Error("declared debug agent-router-update identity required");
  await qualifyArtifact(input.updater);
  const arguments_ = ["--version"];
  await input.receipt.record("rootUpdaterQualificationRequest", {
    executable: input.updater,
    arguments: arguments_,
  });
  const reply = await command(input.updater.path, arguments_, {
    cwd: input.root,
    timeoutSeconds: 30,
  });
  await input.receipt.record("rootUpdaterQualificationReply", reply);
  if (reply.exit !== 0 || reply.stdout.trim() !== `agent-router-update ${input.targetVersion}`)
    throw new Error(
      "root updater version differs from replacement target; old children must remain running",
    );
}

export function requirePreservedRootFiles(
  before: Readonly<Record<string, string>>,
  after: Readonly<Record<string, string>>,
): void {
  if (!isDeepStrictEqual(before, after))
    throw new Error("root identity, enrollment or project files changed during update");
}

async function protectedFiles(root: string): Promise<Record<string, string>> {
  const paths = [
    "agent-communication/service-identity.json",
    "agent-communication/installation-peer.json",
    "fleet/enrollment.json",
  ];
  const projects = `${root}/agent-communication/projects`;
  for (const relative of await readdir(projects, { recursive: true })) {
    const info = await lstat(`${projects}/${relative}`);
    if (info.isSymbolicLink()) throw new Error("symbolic project path refused");
    if (info.isFile()) paths.push(`agent-communication/projects/${relative}`);
  }
  const hashes: Record<string, string> = {};
  for (const relative of paths.sort()) {
    const info = await lstat(`${root}/${relative}`);
    if (!info.isFile() || info.isSymbolicLink() || info.uid !== process.getuid?.())
      throw new Error("owned regular protected root file required");
    hashes[relative] = await fingerprint(`${root}/${relative}`);
  }
  return hashes;
}

export async function updateStoppedInstallationRoot(input: {
  root: string;
  updater: Artifact;
  receipt: Receipt;
  environment: NodeJS.ProcessEnv;
}): Promise<void> {
  const release = await registerCommandRoot(input.root);
  try {
    await qualifyArtifact(input.updater);
    // The source is ordinary JSON. The updater itself confirms its JSONC section through
    // the Host's public settings reader before removing this file (settings_step.rs).
    await requireDisabledUpdateConfiguration(input.root);
    const before = await protectedFiles(input.root);
    await input.receipt.record("rootUpdateBefore", {
      root: input.root,
      protectedFiles: before,
      providersDisabled: true,
      updater: input.updater,
    });
    for (const [phase, arguments_] of [
      ["plan", ["--router-root", input.root, "--dry-run"]],
      ["apply", ["--router-root", input.root]],
      ["verify", ["--router-root", input.root, "--dry-run"]],
    ] as const) {
      await input.receipt.record("rootUpdateRequest", {
        phase,
        executable: input.updater.path,
        arguments: arguments_,
      });
      const reply = await command(input.updater.path, arguments_, {
        cwd: input.root,
        environment: input.environment,
        timeoutSeconds: 300,
      });
      await input.receipt.record("rootUpdateReply", { phase, ...reply });
      if (reply.exit !== 0)
        throw new Error(`qualified root updater ${phase} failed; replacement must not start`);
      if (phase === "verify" && reply.stdout.trim() !== `${input.root} needs no update`)
        throw new Error("root update still has pending work; replacement must not start");
    }
    const settings = await lstat(`${input.root}/settings.jsonc`);
    if (
      !settings.isFile() ||
      settings.isSymbolicLink() ||
      settings.uid !== process.getuid?.() ||
      (settings.mode & 0o077) !== 0
    )
      throw new Error("private carried settings required");
    try {
      await lstat(`${input.root}/providers.json`);
      throw new Error("legacy provider file remains after update");
    } catch (error: unknown) {
      if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
    }
    const after = await protectedFiles(input.root);
    requirePreservedRootFiles(before, after);
    await qualifyArtifact(input.updater);
    await input.receipt.record("rootUpdateComplete", {
      root: input.root,
      protectedFiles: after,
      settingsSha256: await fingerprint(`${input.root}/settings.jsonc`),
      providerIsolation:
        "source disabled; updater confirmed carried values through Host settings reader before removal",
      noEnrollmentInvoked: true,
    });
  } finally {
    release();
  }
}
