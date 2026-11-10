// S1's temporary owner-only observation seam. Object names come from existing
// registry records; the request cannot select an arbitrary namespace or SQL.

import type { CallAnswer, StorageObjects } from "./migration_calls.ts";
import { serviceId } from "./message_board_values.ts";
import { invalid, string, struct, unitEnum, uuidV7 } from "./serde_shapes.ts";
import { parseSnapshotCursor } from "./storage_snapshot.ts";

export const STAGING_SNAPSHOT_PATH = "/admin/v1/migration/staging-snapshot";
export const STAGING_SNAPSHOT_WORKER = "collaboration-central-s1-migration";

const requestValue = struct(
  {
    kind: unitEnum(["fleet", "registry", "copy", "connection", "mailbox"] as const),
    name: string,
    cursor: parseSnapshotCursor,
  },
  true,
);

export async function stagingSnapshot(body: unknown, objects: StorageObjects): Promise<CallAnswer> {
  let request;
  try {
    request = requestValue(body, "body");
    if (request.kind === "fleet" && request.name !== "fleet")
      invalid("body.name", "expected fleet");
    if (request.kind === "registry" && request.name !== "registry")
      invalid("body.name", "expected registry");
    if (request.kind === "copy") uuidV7(request.name, "body.name");
    if (request.kind === "connection" || request.kind === "mailbox") {
      const refusal = serviceId(request.name);
      if (refusal !== null) invalid("body.name", refusal);
    }
  } catch (error) {
    return {
      status: 400,
      body: { refused: "body", reason: error instanceof Error ? error.message : String(error) },
    };
  }
  const unknown = (): CallAnswer => ({
    status: 404,
    body: { refused: "object", reason: "not in the registry cohort" },
  });
  try {
    let binding;
    switch (request.kind) {
      case "fleet":
        binding = objects.namespaces.FLEET;
        break;
      case "registry":
        binding = objects.namespaces.LEASE_REGISTRY;
        break;
      case "copy":
        if (!(await objects.projects()).includes(request.name)) return unknown();
        binding = objects.namespaces.PROJECT_COPIES;
        break;
      case "connection":
      case "mailbox": {
        const registry = await objects.registry(["machine_enrollments"]);
        if (
          !(registry.rows.machine_enrollments ?? []).some((row) => row.service_id === request.name)
        )
          return unknown();
        binding =
          request.kind === "connection"
            ? objects.namespaces.CONNECTIONS
            : objects.namespaces.MAILBOXES;
        break;
      }
    }
    const page = await objects.snapshot(binding, request.name, request.cursor);
    return {
      status: 200,
      body: { ...page, rowsWritten: objects.rowsWritten, queries: objects.readQueries },
    };
  } catch (error) {
    return {
      status: 400,
      body: { refused: "snapshot", reason: error instanceof Error ? error.message : String(error) },
    };
  }
}
