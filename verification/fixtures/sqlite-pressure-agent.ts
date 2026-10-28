import { DatabaseSync } from "node:sqlite";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { requireArgument } from "./acp-stdio.ts";

const mode = z.enum(["reset", "hold"]).parse(requireArgument(2));
const connection = new DatabaseSync(requireArgument(3), { timeout: 0 });
try {
  connection.exec("PRAGMA foreign_keys = OFF");
  if (mode === "hold") {
    const seconds = z.coerce.number().nonnegative().parse(requireArgument(4));
    connection.exec("BEGIN IMMEDIATE");
    console.log("acquired");
    await setTimeout(seconds * 1000);
    connection.exec("COMMIT");
  } else {
    // Python sqlite3 starts a deferred transaction at the first mutation.
    if (process.argv.length > 4) {
      const statement = connection.prepare(
        "DELETE FROM route_band_account_states WHERE account_id = ? AND route_band = 'responses'",
      );
      connection.exec("BEGIN");
      for (const accountId of process.argv.slice(4)) statement.run(accountId);
      connection.exec("COMMIT");
    }
  }
} finally {
  connection.close();
}
