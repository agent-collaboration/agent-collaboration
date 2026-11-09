// The S4 refusal allocation's owner routes (T11). They answer only when this
// deployment's STAGING_SEED_TARGET is the literal S4 allocation, so neither the
// S1 seed nor any other deployment can reach them; the caller is already the
// owner (worker.ts checks Access first). Each names one object of the cohort by
// binding and name and forwards to its S4 operation (s4_fixture_storage.ts).

import type { DurableObjectNamespace } from "./platform.ts";
import { S4_RESTORE_URL, S4_SNAPSHOT_URL, S4_STATEMENTS_URL } from "./storage_objects.ts";

/** The only allocation these routes run in. */
export const S4_ALLOCATION = "collaboration-central-s4-refusals";

const OPERATIONS: Readonly<Record<string, string>> = {
  "/admin/v1/staging-seed/s4/statements": S4_STATEMENTS_URL,
  "/admin/v1/staging-seed/s4/snapshot": S4_SNAPSHOT_URL,
  "/admin/v1/staging-seed/s4/restore": S4_RESTORE_URL,
};
const BINDINGS = ["LEASE_REGISTRY", "FLEET", "PROJECT_COPIES", "CONNECTIONS", "MAILBOXES"] as const;
type Binding = (typeof BINDINGS)[number];

export const isS4Route = (path: string): boolean => path in OPERATIONS;

/** Forwards one S4 call, or refuses it outside the S4 allocation or for an unknown object. */
export async function s4Route(
  path: string,
  body: unknown,
  env: { readonly STAGING_SEED_TARGET?: string } & Readonly<
    Record<Binding, DurableObjectNamespace>
  >,
): Promise<Response> {
  const json = (value: unknown, status: number) =>
    new Response(JSON.stringify(value), {
      status,
      headers: { "content-type": "application/json" },
    });
  if (env.STAGING_SEED_TARGET !== S4_ALLOCATION)
    return json({ refused: "S4 routes run only in the S4 refusal allocation" }, 403);
  const operation = OPERATIONS[path];
  const request = body as { binding?: unknown; name?: unknown; payload?: unknown } | null;
  const binding = BINDINGS.find((name) => name === request?.binding);
  if (operation === undefined || binding === undefined || typeof request?.name !== "string")
    return json({ refused: "name one cohort object by binding and name" }, 400);
  const namespace = env[binding];
  const stub = namespace.get(namespace.idFromName(request.name));
  const answer = await stub.fetch(operation, {
    method: "POST",
    body: JSON.stringify(request.payload ?? null),
  });
  return new Response(await answer.text(), {
    status: answer.status,
    headers: { "content-type": "application/json" },
  });
}
