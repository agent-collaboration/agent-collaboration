import { readFileSync } from "node:fs";
import { decodeEvidenceJson } from "../evidence-json.ts";
import { boardPlanSchema } from "./tailnet-plans.ts";
import { runPeer } from "./tailnet-board-peer.ts";
import { runReadback } from "./tailnet-board-readback.ts";

export async function main(): Promise<void> {
  const [action, path, ...rest] = process.argv.slice(2);
  if (action === "--help" || action === "-h") {
    console.log("usage: tailnet-mcp.ts {peer,readback} PLAN.json");
    return;
  }
  if (path === undefined || rest.length > 0 || (action !== "peer" && action !== "readback"))
    throw new Error(
      "peer or readback action and plan are required; the Host uses the Rust verification executable",
    );
  const plan = boardPlanSchema.parse(decodeEvidenceJson(readFileSync(path)));
  if (action === "peer") await runPeer(plan);
  else await runReadback(plan);
}
if (import.meta.main)
  main().catch((error: unknown): void => {
    console.error(error);
    process.exitCode = 1;
  });
