import { execFileSync } from "node:child_process";
import { realpathSync, writeFileSync } from "node:fs";
import { z } from "zod";
import { AcpExchange } from "./acp-exchange.ts";
import { wireJson } from "./acp-stdio.ts";

export async function runFixture(_configuration: unknown): Promise<void> {
  const agent = new AcpExchange();
  const initialized = await agent.read("initialize");
  const selected = z.string().parse(process.env.CLAUDE_CODE_EXECUTABLE || process.argv[2]);
  const receipt = z.string().parse(process.env.GLOBAL_HARNESS_FIXTURE_ADAPTER_RECEIPT);
  writeFileSync(
    receipt,
    wireJson({ selected_executable: realpathSync(selected), version_stdout: "" }),
  );
  const output = execFileSync(selected, ["--version"], {
    encoding: "utf8",
    maxBuffer: Number.MAX_SAFE_INTEGER,
  }).replace(/\r\n?/g, "\n");
  writeFileSync(
    receipt,
    wireJson({ selected_executable: realpathSync(selected), version_stdout: output.trim() }),
  );
  agent.respond(initialized, {
    protocolVersion: 1,
    agentCapabilities: {},
    agentInfo: { name: "global-harness-selection-fixture", version: "1" },
  });
  await agent.drain();
  agent.close();
}
