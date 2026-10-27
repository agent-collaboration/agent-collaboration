import { once } from "node:events";
import { createConnection } from "node:net";
import { z } from "zod";
import { wireJson } from "./acp-stdio.ts";

async function main(): Promise<void> {
  const mode = z.string().parse(process.env.TAILNET_TEST_MODE);
  if (mode === "finite_failure") {
    console.error("PRIVATE_FIXTURE_OUTPUT");
    process.exitCode = 23;
    return;
  }
  if (mode === "finite_overflow") {
    console.log("PRIVATE_FIXTURE_OUTPUT".repeat(65537));
    return;
  }
  if (mode === "finite_hang") {
    setInterval((): void => {}, 60_000);
    return;
  }
  if (process.argv.slice(2).join("\0") === "status\0--json") {
    if (mode === "overflow") console.log("x".repeat(65537));
    else if (mode === "bad_status") console.log("not-json");
    else
      console.log(
        wireJson({
          BackendState: "Running",
          Self: { DNSName: "node.example.ts.net." },
          CertDomains: ["node.example.ts.net"],
        }),
      );
    return;
  }
  if (mode === "early_exit") {
    process.exitCode = 23;
    return;
  }
  process.on("SIGINT", (): never => process.exit(0));
  console.log("https://node.example.ts.net:8443/");
  if (mode === "controlled_exit") {
    const socket = createConnection(z.string().parse(process.env.TAILNET_TEST_CONTROL));
    try {
      await once(socket, "connect");
      socket.write("ready");
      await new Promise<void>((resolve, reject): void => {
        socket.once("data", (): void => {
          resolve();
        });
        socket.once("end", resolve);
        socket.once("error", reject);
      });
    } finally {
      socket.destroy();
    }
    process.exitCode = 23;
    return;
  }
  setInterval((): void => {}, 60_000);
}
await main();
