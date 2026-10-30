import assert from "node:assert/strict";
import { appendFileSync } from "node:fs";
import { z } from "zod";
import { requireArgument, wireJson } from "./acp-stdio.ts";
import { sourceShellWords } from "./source-shell-words.ts";
import { field, textField } from "../exact-json.ts";
import { readObservedJson } from "../observations/evidence-models.ts";

/** An older executor that would resume blindly if it receives a mutation. */
export function runFixture(configuration: unknown): void {
  const fixture = z
    .object({
      clientInventory: z.record(z.string(), z.unknown()),
      page: z.record(z.string(), z.unknown()),
    })
    .parse(configuration);
  assert.deepEqual(process.argv.slice(2, 11), [
    "-T",
    "-o",
    "BatchMode=yes",
    "-o",
    "StrictHostKeyChecking=yes",
    "-o",
    "ConnectTimeout=5",
    "--",
    "source.example.invalid",
  ]);
  const parts = sourceShellWords(requireArgument(11));
  assert.deepEqual(parts.slice(0, 3), ["exec", "/fixed/agent-sessions", "--source-request-v1"]);
  const request = readObservedJson(z.string().parse(parts[3]));
  const operation = textField(request, "operation", "kind");
  appendFileSync(z.string().parse(process.env.SOURCE_SSH_COUNT), `${operation}\n`);
  let reply: unknown;
  switch (operation) {
    case "clientInventory":
      reply = fixture.clientInventory;
      break;
    case "providerSessions":
      reply = { kind: "providerSessions", page: fixture.page };
      break;
    case "nativeClaude": {
      const action = readObservedJson(wireJson(field(request, "operation", "action")));
      delete action.arguments;
      reply = {
        kind: "nativeClaude",
        result: {
          source: {
            serviceId: request.expectedService,
            serviceEpoch: field(request, "operation", "serviceEpoch"),
          },
          action,
          observation: {
            kind: "observed",
            status: { kind: "exited", code: 0 },
            stdout: [...Buffer.from("backgrounded · deadbeef · fixture\n")],
            stderr: [],
          },
        },
      };
      break;
    }
    default:
      throw new Error(`unexpected source operation: ${operation}`);
  }
  console.log(wireJson({ kind: "ready", reply }));
}
