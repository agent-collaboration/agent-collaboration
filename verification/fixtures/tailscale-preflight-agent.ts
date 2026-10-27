import { z } from "zod";
import { wireJson } from "./acp-stdio.ts";

const mode = z.string().parse(process.env.TAILNET_TEST_MODE);
const arguments_ = process.argv.slice(2).join("\0");
if (arguments_ === "status\0--json") {
  if (mode === "malformed") console.log("not-json");
  else
    console.log(
      wireJson({
        BackendState: mode === "offline" ? "Stopped" : "Running",
        Self: { DNSName: "node.example.ts.net." },
        CertDomains: mode === "no_https" ? [] : ["node.example.ts.net"],
      }),
    );
} else if (arguments_ === "serve\0status\0--json")
  console.log(
    wireJson(
      mode === "collision" ? { Foreground: { foreign: { TCP: { "8443": { HTTPS: true } } } } } : {},
    ),
  );
else process.exitCode = 73;
