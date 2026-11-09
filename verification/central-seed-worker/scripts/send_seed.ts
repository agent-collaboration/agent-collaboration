// Sends seed requests built by build_seed_request.ts to a deployed staging seed
// Worker. Standalone (node only), so each Mac can seed its own projects without
// its exports leaving it. The owner's Access token comes from the environment
// (CENTRAL_SEED_ACCESS_TOKEN, e.g. from `cloudflared access token`) and is
// never printed.
//
//   node scripts/send_seed.ts copies   <staging origin> <copies-NNN.json>...
//   node scripts/send_seed.ts registry <staging origin> <registry.json>...
//
// The registry phase merges every registry.json it is given (one per Mac) into
// one request: same target and incarnation, each project once.

import { readFile } from "node:fs/promises";
import type { RegistryRequest } from "../src/seed_contract.ts";

const [phase, origin, ...files] = process.argv.slice(2);
if ((phase !== "copies" && phase !== "registry") || origin === undefined || files.length === 0) {
  throw new Error("usage: send_seed.ts <copies|registry> <staging origin> <request files...>");
}
const token = process.env.CENTRAL_SEED_ACCESS_TOKEN;
if (token === undefined || token === "") throw new Error("CENTRAL_SEED_ACCESS_TOKEN is not set");
const url = new URL(`/admin/v1/staging-seed/${phase}`, origin);
if (url.protocol !== "https:") throw new Error("the staging origin must be https");

async function send(body: string): Promise<unknown> {
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json", "cf-access-token": token ?? "" },
    body,
  });
  const answer: unknown = await response.json().catch(() => ({ status: response.status }));
  if (!response.ok)
    throw new Error(`${url.pathname} answered ${response.status}: ${JSON.stringify(answer)}`);
  return answer;
}

if (phase === "copies") {
  for (const file of files) {
    console.log(JSON.stringify({ file, answer: await send(await readFile(file, "utf8")) }));
  }
} else {
  const requests = await Promise.all(
    files.map(async (file) => JSON.parse(await readFile(file, "utf8")) as RegistryRequest),
  );
  const [first] = requests;
  if (first === undefined) throw new Error("no registry requests");
  if (
    requests.some(
      (request) =>
        request.target !== first.target ||
        request.registryIncarnation !== first.registryIncarnation,
    )
  ) {
    throw new Error("registry requests name different targets or incarnations");
  }
  const merged: RegistryRequest = {
    target: first.target,
    registryIncarnation: first.registryIncarnation,
    projects: requests.flatMap((request) => request.projects),
  };
  const seen = new Set(merged.projects.map((project) => project.database));
  if (seen.size !== merged.projects.length)
    throw new Error("a project appears in two registry requests");
  console.log(
    JSON.stringify({
      projects: merged.projects.length,
      answer: await send(JSON.stringify(merged)),
    }),
  );
}
