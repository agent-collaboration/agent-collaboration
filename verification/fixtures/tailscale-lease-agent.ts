import { readFileSync, writeFileSync } from "node:fs";
import { z } from "zod";
import { wireJson } from "./acp-stdio.ts";

const path = z.string().parse(process.env.TAILNET_TEST_STATE);
if (process.argv.slice(2).join("\0") === "serve\0status\0--json")
  console.log(readFileSync(path, "utf8").replace(/\r\n?/g, "\n"));
else {
  const foreground: Record<string, unknown> = {
    foreign: { TCP: { "443": { HTTPS: true } } },
    "fixture-owned": { TCP: { "8443": { HTTPS: true } } },
  };
  const state = { Foreground: foreground };
  writeFileSync(path, wireJson(state));
  process.on("SIGINT", (): never => {
    if (process.env.TAILNET_TEST_KEEP_LEASE !== "yes") {
      delete foreground["fixture-owned"];
      writeFileSync(path, wireJson(state));
    }
    process.exit(0);
  });
  console.log("https://node.example.ts.net:8443/");
  setInterval((): void => {}, 60_000);
}
