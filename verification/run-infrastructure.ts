import { randomBytes, randomUUID } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import type { Declaration } from "./declaration.ts";
import { ObservationFailure, type Receipt } from "./receipt.ts";
import type { RunLifetime } from "./run-lifetime.ts";
import type { Installation } from "./machines/installation.ts";
import { Link } from "./machines/link.ts";
import { TokioCentral } from "./targets/tokio-central.ts";
import { WorkerdCentral } from "./targets/workerd-central.ts";
import { StagingCentral } from "./targets/staging-central.ts";
import type { CentralTarget } from "./targets/central-target.ts";
import { FixtureEdge } from "./access/fixture-edge.ts";
import { SessionGate } from "./access/session-gate.ts";
import { fixtureMachineAssertion } from "./access/fixture-assertion.ts";
import { enrollInstallation } from "./clients/fleet-enrollment.ts";
import { environmentValue } from "./clients/central-owner.ts";

export async function startCentral(
  declaration: Pick<Declaration, "repository" | "target">,
  receipt: Receipt,
): Promise<CentralTarget> {
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
export async function enroll(input: {
  installation: Installation;
  declaration: Pick<Declaration, "target" | "deadline" | "repository">;
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
