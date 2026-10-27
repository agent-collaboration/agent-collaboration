import { wireJson } from "./acp-stdio.ts";
const arguments_ = process.argv.slice(2).join("\0");
if (arguments_ === "status\0--json")
  console.log(
    wireJson({
      BackendState: "Running",
      Self: { DNSName: "node.example.ts.net." },
      CertDomains: ["node.example.ts.net"],
    }),
  );
else if (arguments_ === "serve\0status\0--json") console.log("{}");
else process.exitCode = 23;
