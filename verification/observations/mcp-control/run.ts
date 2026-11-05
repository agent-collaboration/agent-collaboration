// Entry point: `node verification/observations/mcp-control/run.ts PLAN.json [--check]`.
// --check validates the declared target and apparatus and contacts nothing (control-plan.ts).
// Without it, the run speaks only ordinary MCP to the declared Host: initialize, tools/list and
// the declared cases. It exits 0 only when every case passes; the first case that does not stops
// all later mutating input, and its IDs and known targets stay in the evidence.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { decodeAuthoredJson, encode } from "../../exact-json.ts";
import { loadAcpInventory, toolCoverageDefects } from "./acp-inventory.ts";
import {
  checkControlPlan,
  controlPlanSchema,
  type ControlCase,
  type ControlPlan,
} from "./control-plan.ts";
import {
  callerLossCase,
  casePassed,
  explicitCancelCase,
  lifecycleCase,
  type CaseInputs,
  type CaseResult,
} from "./journey-cases.ts";
import { loadResumeCase, staleCancelCase, wrongRoleCase } from "./journey-control-cases.ts";
import { openEvidence, record, type JourneyContext } from "./journey-steps.ts";
import { ProviderStoreReadback } from "./role-readback.ts";
import { StreamingMcpClient } from "./streaming-mcp-client.ts";
import { z } from "zod";

const CASES: Readonly<Record<ControlCase, (inputs: CaseInputs) => Promise<CaseResult>>> = {
  lifecycle: lifecycleCase,
  callerLoss: callerLossCase,
  explicitCancel: explicitCancelCase,
  staleCancel: staleCancelCase,
  wrongRole: wrongRoleCase,
  loadResume: loadResumeCase,
};

async function discover(context: JourneyContext): Promise<readonly string[]> {
  const initialized = await context.client.initialize(15_000);
  record(context, { step: "initialize", outcome: initialized });
  if (initialized.kind !== "answered") throw new Error(`MCP initialize was ${initialized.kind}`);
  const listed = await context.client.listTools(15_000);
  record(context, { step: "tools/list", outcome: listed });
  if (listed.kind !== "answered") throw new Error(`tools/list was ${listed.kind}`);
  const names = z
    .object({ tools: z.array(z.looseObject({ name: z.string() })) })
    .parse(listed.value)
    .tools.map((tool) => tool.name);
  const missing = toolCoverageDefects(loadAcpInventory(), names);
  if (missing.length > 0) throw new Error(`inventory: ${missing.join("; ")}`);
  return names;
}

async function runCases(
  plan: ControlPlan,
  context: JourneyContext,
  store: ProviderStoreReadback,
): Promise<CaseResult[]> {
  const inputs: CaseInputs = {
    context,
    plan,
    store,
    promptText: plan.promptText,
    holdText: plan.holdText ?? "",
  };
  const results: CaseResult[] = [];
  for (const [index, name] of plan.cases.entries()) {
    let result: CaseResult;
    try {
      result = await CASES[name](inputs);
    } catch (error) {
      // A case that cannot reach its deciding observation is a finding, never retried.
      result = { name, target: null, verdict: { kind: "failed", reason: String(error) } };
    }
    results.push(result);
    record(context, { step: `verdict-${name}`, result });
    if (!casePassed(result.verdict)) {
      // An unconfirmed or failed case may leave admitted work or a held turn behind, so no later
      // case sends mutating input. Its operation IDs and target stay in exchanges.jsonl.
      record(context, { step: "stopped", after: name, notRun: plan.cases.slice(index + 1) });
      break;
    }
  }
  return results;
}

async function main(): Promise<number> {
  const [planPath, flag] = process.argv.slice(2);
  if (planPath === undefined || (flag !== undefined && flag !== "--check")) {
    process.stderr.write("usage: run.ts PLAN.json [--check]\n");
    return 2;
  }
  const plan = controlPlanSchema.parse(decodeAuthoredJson(readFileSync(planPath, "utf8")));
  const target = checkControlPlan(plan, new Date());
  if (flag === "--check") {
    process.stdout.write(
      `${encode({ checked: true, mcpEndpoint: target.mcpEndpoint, cases: plan.cases })}\n`,
    );
    return 0;
  }
  openEvidence(plan.evidence);
  const context: JourneyContext = {
    client: new StreamingMcpClient(target.mcpEndpoint),
    evidence: plan.evidence,
    deadline: Date.parse(plan.deadline),
  };
  const store = new ProviderStoreReadback(target.providerStore);
  try {
    await discover(context);
    const results = await runCases(plan, context, store);
    const summary = {
      results,
      notRun: plan.cases.slice(results.length),
      knownTargets: results.flatMap((result) => (result.target === null ? [] : [result.target])),
      nonTerminalOperations: store.nonTerminalCount(),
    };
    writeFileSync(join(plan.evidence, "verdicts.json"), `${encode(summary)}\n`, {
      flag: "wx",
      mode: 0o600,
    });
    process.stdout.write(`${encode(summary)}\n`);
    const allPassed =
      summary.notRun.length === 0 && results.every((result) => casePassed(result.verdict));
    return allPassed && summary.nonTerminalOperations === 0 ? 0 : 1;
  } finally {
    store.close();
  }
}

process.exitCode = await main();
