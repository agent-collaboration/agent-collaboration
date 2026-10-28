import { createHash } from "node:crypto";
import { existsSync, mkdirSync } from "node:fs";
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { arrayField } from "../exact-json.ts";
import { evidenceJson } from "../evidence-json.ts";
import { sameJsonContent } from "../json-comparison.ts";
import { splitTextLines } from "../text-lines.ts";
import { requireAdvertisedSettings } from "./callback-oracles.ts";
import {
  endpointInventorySchema,
  observedObject,
  operationViewSchema,
  providerRecordSchema,
  readObservedJson,
} from "./evidence-models.ts";
import {
  approverFor,
  callerFor,
  journeyRecord,
  manifestFor,
  operationId,
  providerRecord,
  type JourneyPlan,
} from "./provider-journey-plan.ts";
import { rpcCall } from "./provider-journey-rpc.ts";
import { registerGuardIdentifiers } from "./readonly-codex-home.ts";
import { processExit, ProcessWaitTimeout, saveRecord } from "./runtime-plan.ts";

interface CreateCommandResult {
  readonly exit: number;
  readonly stdout: string;
  readonly stderr: string;
}
function runCreateCommand(command: readonly string[]): Promise<CreateCommandResult> {
  const [binary, ...arguments_] = command;
  if (binary === undefined) throw new Error("create executable missing");
  return new Promise((resolve, reject): void => {
    const child = spawn(binary, arguments_, { stdio: ["inherit", "pipe", "pipe"] });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    let timedOut = false;
    const timer = setTimeout((): void => {
      timedOut = true;
      child.kill("SIGKILL");
    }, 75_000);
    child.stdout.on("data", (chunk: Buffer): void => {
      stdout.push(chunk);
    });
    child.stderr.on("data", (chunk: Buffer): void => {
      stderr.push(chunk);
    });
    child.on("error", (error): void => {
      clearTimeout(timer);
      reject(error);
    });
    child.on("close", (): void => {
      clearTimeout(timer);
      if (timedOut) {
        reject(new ProcessWaitTimeout("create command timed out; no replay"));
        return;
      }
      const decode = (parts: readonly Buffer[]): string =>
        new TextDecoder("utf-8", { fatal: true })
          .decode(Buffer.concat(parts))
          .replace(/\r\n?/g, "\n");
      try {
        const exit = processExit(child);
        if (exit === null) throw new Error("create command exit unavailable");
        resolve({ exit, stdout: decode(stdout), stderr: decode(stderr) });
      } catch (error) {
        reject(error);
      }
    });
  });
}

export async function createSession(plan: JourneyPlan): Promise<void> {
  mkdirSync(plan.evidence, { mode: 0o700, recursive: true });
  if (existsSync(join(plan.evidence, "create-intent.json")))
    throw new Error("create already attempted; inspect retained operation, never replay");
  const manifest = manifestFor(plan);
  const caller = callerFor(plan);
  const operation = operationId();
  const rawInventory = await rpcCall(plan, "endpoints_list", {});
  const inventory = endpointInventorySchema.parse(rawInventory);
  const index = inventory.endpoints.findIndex((item) =>
    sameJsonContent(item.endpoint, { serviceId: manifest.serviceId, endpointId: plan.endpoint_id }),
  );
  const endpoint = inventory.endpoints[index];
  if (endpoint === undefined || endpoint.availability.state !== "available")
    throw new Error("selected provider endpoint unavailable; no create");
  const channel = endpoint.channels.find((item) => item.kind === "externalProvider");
  if (channel === undefined || channel.bindingGeneration === null)
    throw new Error("selected endpoint is not an admitted external provider");
  saveRecord(
    join(plan.evidence, "endpoint-preflight.json"),
    arrayField(rawInventory, "endpoints")[index],
  );
  const generation = { serviceEpoch: manifest.serviceEpoch, generation: channel.bindingGeneration };
  const argv = [
    plan.cli,
    "conversation",
    "create",
    "--endpoint",
    plan.endpoint_id,
    "--cwd",
    join(dirname(plan.evidence), "workspace"),
    "--access",
    "write-restricted",
    "--from",
    evidenceJson(caller),
    "--approver",
    evidenceJson(approverFor(plan)),
    "--generation",
    evidenceJson(generation),
    "--operation-id",
    operation,
    "--timeout-seconds",
    "60",
    "--service-directory",
    plan.service_directory,
    "--json",
  ];
  for (const [name, value] of Object.entries(plan.settings)) {
    if (!["mode", "model", "effort"].includes(name))
      throw new Error("setting has no supported CLI selector; no create");
    argv.push(`--${name}`, value);
  }
  saveRecord(join(plan.evidence, "create-intent.json"), {
    operationId: operation,
    argv,
    creator: caller,
    approver: approverFor(plan),
    testAutomation: true,
  });
  const result = await runCreateCommand(argv);
  const values = splitTextLines(result.stdout)
    .filter((line) => line.trim())
    .map(readObservedJson);
  if (result.exit !== 0) {
    saveRecord(join(plan.evidence, "create-failure.json"), {
      exit: result.exit,
      stdout: values,
      stderrSHA256: createHash("sha256").update(result.stderr).digest("hex"),
      doNotReplay: true,
    });
    throw new Error("create failed; reconcile operation, no second create");
  }
  const created = values.at(-1);
  if (created === undefined) throw new Error("create returned no JSON result; no replay");
  saveRecord(join(plan.evidence, "create-result.json"), created);
  await inspectCreatedSession(plan);
}

export async function inspectCreatedSession(plan: JourneyPlan): Promise<void> {
  const intent = journeyRecord(plan, "create-intent");
  let operation;
  if (existsSync(join(plan.evidence, "create-operation-readback.json")))
    operation = journeyRecord(plan, "create-operation-readback");
  else {
    operation = await rpcCall(plan, "conversation_operation_show", {
      operationId: intent.operationId,
    });
    saveRecord(join(plan.evidence, "create-operation-readback.json"), operation);
  }
  const snapshot = operationViewSchema.parse(operation);
  if (
    snapshot.stage !== "terminal" ||
    snapshot.effect !== "applied" ||
    snapshot.reconciliation !== "confirmed"
  )
    throw new Error("create operation not confirmed; no replay");
  const target = observedObject(operation.target);
  saveRecord(join(plan.evidence, "target-binding.json"), target);
  registerGuardIdentifiers(dirname(plan.evidence), operation);
  const inspected = await rpcCall(plan, "provider_session_inspect", { target });
  saveRecord(join(plan.evidence, "session-inspect.json"), inspected);
  const raw = providerRecord(inspected);
  const record = providerRecordSchema.parse(raw);
  if (!record.capabilities.elicitation)
    throw new Error("unsupported real provider Question; no model prompt");
  if (Object.keys(plan.settings).length > 0) requireAdvertisedSettings(raw, plan.settings);
  const capabilities = observedObject(raw.capabilities);
  console.log(
    evidenceJson({
      created: target,
      routerElicitationRoute: record.capabilities.elicitation,
      selectedSettings: plan.settings,
      catalogOnly: Object.keys(plan.settings).length === 0,
      authStatus: capabilities.authStatus ?? null,
      providerQuestionToolSupport: "requires qualified tool provenance and genuine emission",
    }),
  );
}
