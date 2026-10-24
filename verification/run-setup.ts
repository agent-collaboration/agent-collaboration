import { randomBytes, randomUUID } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import type { Declaration, Identity } from "./declaration.ts";
import { ObservationFailure, ObservationOpen, type Receipt } from "./receipt.ts";
import { RunLifetime } from "./run-lifetime.ts";
import type { JourneyMachine } from "./run-context.ts";
import { Installation } from "./machines/installation.ts";
import { Link } from "./machines/link.ts";
import { SealedLegacyCopy } from "./inputs/sealed-legacy-copy.ts";
import { TokioCentral } from "./targets/tokio-central.ts";
import { WorkerdCentral } from "./targets/workerd-central.ts";
import { StagingCentral } from "./targets/staging-central.ts";
import type { CentralTarget } from "./targets/central-target.ts";
import { FixtureEdge } from "./access/fixture-edge.ts";
import { SessionGate } from "./access/session-gate.ts";
import { fixtureMachineAssertion } from "./access/fixture-assertion.ts";
import { enrollInstallation } from "./clients/fleet-enrollment.ts";
import { environmentValue } from "./clients/central-owner.ts";
import { runUpgrade, type UpgradeResult } from "./clients/board-upgrade.ts";
import { readLegacyOracle, type LegacyOracle } from "./clients/legacy-oracle.ts";
import { acceptedWrite, boardIdentity, writeHeader } from "./clients/board-requests.ts";
import { RouterNative } from "./clients/router-native.ts";
import { CodexSession } from "./recipients/codex-session.ts";

export interface PreparedRun {
  readonly machines: readonly [JourneyMachine, JourneyMachine];
  readonly central: CentralTarget;
  readonly gates: ReadonlyMap<string, SessionGate>;
  readonly links: readonly Link[];
  readonly copies: readonly SealedLegacyCopy[];
  readonly oracles: readonly LegacyOracle[];
  readonly imported: Extract<UpgradeResult, { kind: "uploaded" }>;
}
async function startCentral(declaration: Declaration, receipt: Receipt): Promise<CentralTarget> {
  switch (declaration.target.kind) {
    case "tokio":
      return TokioCentral.start({
        target: declaration.target,
        repository: declaration.repository,
        receipt,
      });
    case "workerd":
      return WorkerdCentral.start(declaration.target);
    case "staging":
      return new StagingCentral(declaration.target);
  }
}
async function enroll(input: {
  installation: Installation;
  declaration: Declaration;
  central: CentralTarget;
  lifetime: RunLifetime;
  receipt: Receipt;
  gates: Map<string, SessionGate>;
  links: Link[];
}): Promise<void> {
  const { installation, declaration, lifetime, receipt } = input;
  let enrollment;
  let ownerToken: string;
  if (declaration.target.kind === "staging") {
    const binding = installation.machine.enrollment;
    if (binding.kind !== "access") throw new Error("staging requires Access enrollment inputs");
    if (new URL(binding.endpoint).origin !== declaration.target.origin)
      throw new Error("machine endpoint differs from declared central");
    enrollment = binding;
    ownerToken = environmentValue(binding.ownerTokenEnv);
  } else {
    if (installation.machine.enrollment.kind !== "fixture")
      throw new Error("local run requires explicit fixture enrollment");
    const accessClientId = `verification-${randomUUID()}.access`;
    const accessSecret = randomBytes(32).toString("hex");
    const expiresAt = new Date(
      Math.floor(Date.parse(declaration.deadline) / 1000) * 1000,
    ).toISOString();
    ownerToken = randomBytes(32).toString("hex");
    const edge = lifetime.own(
      `edge-${installation.machine.name}`,
      await FixtureEdge.start({
        upstream: input.central.origin,
        assertion: await fixtureMachineAssertion({
          key: declaration.target.fixtureKey,
          accessClientId,
          expiresAt,
        }),
        owner: {
          token: ownerToken,
          assertion: (await readFile(declaration.target.assertionFile, "utf8")).trim(),
        },
        accessId: accessClientId,
        accessSecret,
      }),
    );
    const relative = "fixture-access-secret";
    if (installation.remote === undefined)
      await writeFile(`${installation.machine.root}/${relative}`, accessSecret, {
        mode: 0o600,
        flag: "wx",
      });
    else await installation.remote.writePrivate(relative, accessSecret);
    enrollment = {
      endpoint: `${edge.origin}/v1/session`,
      label: installation.enrollmentLabel,
      accessClientId,
      accessSecretFile: `${installation.machine.root}/${relative}`,
      expiresAt,
      ownerTokenEnv: "HARNESS_FLEET_OWNER_TOKEN",
    };
  }
  const gate = lifetime.own(
    `session-gate-${installation.machine.name}`,
    await SessionGate.start(enrollment.endpoint),
  );
  input.gates.set(installation.machine.name, gate);
  if (installation.remote !== undefined)
    input.links.push(
      lifetime.own(
        `link-${installation.machine.name}`,
        await Link.start({
          remote: installation.remote,
          port: Number(new URL(gate.origin).port),
          receipt,
          repository: declaration.repository,
        }),
      ),
    );
  enrollment = { ...enrollment, endpoint: `${gate.origin}/v1/session` };
  const result = await enrollInstallation({ installation, enrollment, ownerToken, receipt });
  if (result.kind !== "published")
    throw new ObservationFailure("product CLI did not publish enrollment");
}
export async function projectForCases(
  installation: Installation,
  actor: Identity,
): Promise<string> {
  const workspaceId = boardIdentity(),
    boardId = boardIdentity(),
    projectId = boardIdentity();
  for (const [tool, body] of [
    [
      "workspace_create",
      { workspaceId, name: workspaceId, description: "Disposable verification workspace" },
    ],
    [
      "board_create",
      {
        workspaceId,
        boardId,
        name: boardId,
        description: "Verification board",
        kind: { kind: "area" },
      },
    ],
    [
      "project_create",
      {
        boardId,
        projectId,
        title: projectId,
        description: "Live verification records after real import",
      },
    ],
    [
      "role_grant",
      {
        projectId,
        operation: {
          kind: "roleGrant",
          subject: { kind: "project", subject: projectId },
          role: "lead",
          holder: actor,
          model: null,
          lineage: null,
          expectedRevision: 0n,
          ownerGrant: null,
        },
      },
    ],
  ] satisfies readonly (readonly [string, Readonly<Record<string, unknown>>])[])
    await acceptedWrite({
      client: installation.mcp,
      tool,
      request: { ...writeHeader(actor), ...body },
    });
  return projectId;
}
async function startRecipient(input: {
  installation: Installation;
  declaration: Declaration;
  receipt: Receipt;
  lifetime: RunLifetime;
  gates: ReadonlyMap<string, SessionGate>;
  actor: Identity;
}): Promise<JourneyMachine> {
  const { installation, declaration, receipt, lifetime, actor } = input;
  lifetime.afterDependencies(installation);
  await installation.start();
  const gate = input.gates.get(installation.machine.name);
  if (gate === undefined) throw new ObservationOpen("installation session gate absent");
  await receipt.record("initialSessionWelcome", {
    machine: installation.machine.name,
    observation: await gate.ready(),
  });
  const identity = await installation.identity();
  const native = lifetime.own(
    `native-${installation.machine.name}`,
    await RouterNative.connect({
      machine: installation.machine,
      repository: declaration.repository,
      receipt,
    }),
  );
  const recipient = await CodexSession.create({
    connection: native,
    endpoint: { serviceId: identity.serviceId, endpointId: "codex-local" },
    cwd: installation.machine.nativeCwd,
    recipient: declaration.recipient,
    receipt,
  });
  return {
    installation,
    recipient,
    actor,
    heldProject: await projectForCases(installation, actor),
  };
}
export async function prepareRun(input: {
  declaration: Declaration;
  receipt: Receipt;
  lifetime: RunLifetime;
}): Promise<PreparedRun> {
  const { declaration, receipt, lifetime } = input;
  const copies: SealedLegacyCopy[] = [];
  const oracles: LegacyOracle[] = [];
  lifetime.own("sealed-inputs", {
    close: async (): Promise<void> => {
      const results = await Promise.allSettled(copies.map((copy): Promise<void> => copy.verify()));
      if (results.some((result): boolean => result.status === "rejected"))
        throw new Error("sealed source integrity changed");
    },
  });
  for (const source of declaration.legacyStores) {
    const copy = await SealedLegacyCopy.qualify(source);
    copies.push(copy);
    if (declaration.rows.includes("OB5"))
      oracles.push(await readLegacyOracle({ copy, artifact: source.oracle, receipt }));
  }
  const central = lifetime.own("central", await startCentral(declaration, receipt));
  const installations: Installation[] = [];
  const gates = new Map<string, SessionGate>();
  const links: Link[] = [];
  for (const machine of declaration.machines) {
    const installation = lifetime.own(
      `installation-${machine.name}`,
      await Installation.prepare({
        machine,
        repository: declaration.repository,
        productVersion: declaration.productVersion,
        receipt,
      }),
    );
    installations.push(installation);
    await enroll({ installation, declaration, central, receipt, lifetime, gates, links });
  }
  const local = installations.find((installation): boolean => installation.remote === undefined);
  const remote = installations.find((installation): boolean => installation.remote !== undefined);
  if (local === undefined || remote === undefined)
    throw new Error("two independent installations required");
  const importInput = { artifact: declaration.importer, installation: local, copies, receipt };
  const dry = await runUpgrade({ ...importInput, publication: { kind: "dryRun" } });
  if (dry.kind !== "dryRun") throw new ObservationFailure("real sealed input dry run refused");
  const authentication =
    declaration.target.kind === "staging"
      ? ({ kind: "ownerAccess" } satisfies { kind: "ownerAccess" })
      : ({ kind: "fixture", assertionFile: declaration.target.assertionFile } satisfies {
          kind: "fixture";
          assertionFile: string;
        });
  const imported = await runUpgrade({
    ...importInput,
    publication: { kind: "upload", centralURL: central.origin, authentication },
  });
  if (imported.kind !== "uploaded")
    throw new ObservationFailure("real sealed input publication refused");
  await receipt.equal(
    "dry run and publication preserve fleet content",
    imported.report.fleetDigest,
    dry.report.fleetDigest,
  );
  const actors: JourneyMachine[] = [];
  for (const installation of [local, remote])
    actors.push(
      await startRecipient({
        installation,
        declaration,
        receipt,
        lifetime,
        gates,
        actor: { kind: "human", humanId: imported.report.owner },
      }),
    );
  const first = actors[0],
    second = actors[1];
  if (first === undefined || second === undefined)
    throw new ObservationOpen("recipient pair not prepared");
  return { machines: [first, second], central, gates, links, copies, oracles, imported };
}
