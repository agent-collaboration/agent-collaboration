// Starts the established loader. Requests and business assertions belong to Rust.
import { createInterface } from "node:readline";
import {
  WorkerdStorage,
  centralTestBundle,
} from "../../../../verification/targets/workerd-storage.ts";
const bundle = centralTestBundle();
const host = await WorkerdStorage.start({
  ...bundle,
  durableObjects: { ...bundle.durableObjects, CENTRAL: "CentralObject" },
});
process.stdout.write(`${JSON.stringify({ origin: host.origin })}\n`);
try {
  const commands = createInterface({ input: process.stdin, terminal: false });
  for await (const command of commands) {
    if (command !== "close") throw new Error("unknown bridge control");
    commands.close();
    break;
  }
} finally {
  await host.close();
}
