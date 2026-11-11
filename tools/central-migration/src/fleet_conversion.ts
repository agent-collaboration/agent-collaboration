// Production's Fleet graph rows → main's typed rows (PD4), following the
// column map in tmp/2026-11-09-migration/column-map-fleet.md. Pure: every
// value is checked before any write, and a value main's types reject stops the
// run with the row named (R7) instead of being rewritten.

import { I64_MAX, UnpairedSurrogate, parseSerdeJson } from "./exact_json.ts";
import {
  canonicalRouterTime,
  commitSha,
  commonDirectory,
  description as descriptionText,
  endpointId,
  humanId,
  machineLabel,
  markdownText,
  normalizedOrigin,
  resourceName,
  serviceId,
  sessionId,
} from "./message_board_values.ts";
import type { ReadRow as SqlRow } from "./storage_objects.ts";

export class Rejected extends Error {}
const reject = (where: string, why: string): never => {
  throw new Rejected(`${where}: ${why}`);
};

const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
export const isUuidV7 = (value: unknown): value is string =>
  typeof value === "string" && UUID_V7.test(value);

function text(value: unknown, where: string): string {
  return typeof value === "string" ? value : reject(where, "expected text");
}
/** Text main decodes into a validated type through `canonical`: refused unless the type keeps it exactly. */
function typed(value: unknown, where: string, check: (value: string) => string | null): string {
  const result = text(value, where);
  const refusal = check(result);
  return refusal === null ? result : reject(where, refusal);
}
function optionalTyped(
  value: unknown,
  where: string,
  check: (value: string) => string | null,
): string | null {
  return value === undefined || value === null ? null : typed(value, where, check);
}
/** message_board RecordKind (record_references.rs:14–37), as fleet_adr_codec.rs:181 decodes made_in_kind. */
const RECORD_KINDS = [
  "workspace",
  "board",
  "adr",
  "project",
  "workstream",
  "task",
  "taskLinkEvent",
  "subtask",
  "thread",
  "threadRefiled",
  "post",
  "roleAssignment",
  "ownerGrant",
  "standDownAcknowledgement",
  "fact",
  "planRevision",
  "readMark",
  "notifyIntent",
  "subscriptionMembership",
  "request",
  "workTrailRow",
  "trailRedaction",
] as const;

/** A stored JSON text read as serde_json reads it: an unpaired surrogate escape is refused. */
function storedJson(value: unknown, where: string): unknown {
  try {
    return parseSerdeJson(text(value, where));
  } catch (error) {
    if (error instanceof Rejected) throw error;
    return reject(where, error instanceof UnpairedSurrogate ? error.message : "not JSON");
  }
}

/** fleet-admission FleetImportDigest: a CanonicalHash, 64 lowercase hexadecimal characters. */
const importDigest = (value: string): string | null =>
  /^[0-9a-f]{64}$/.test(value) ? null : "must be 64 lowercase hexadecimal characters";
function uuid(value: unknown, where: string): string {
  return isUuidV7(value) ? value : reject(where, "expected a canonical UUIDv7");
}
/** A nonnegative integer main stores in an i64 column, kept exact (a bigint beyond 2^53). */
function count(value: unknown, where: string): number | bigint {
  if (typeof value === "number" && Number.isSafeInteger(value) && value >= 0) return value;
  if (typeof value === "bigint" && value >= 0n && value <= I64_MAX) return value;
  return reject(where, "expected a nonnegative integer within i64");
}
function oneOf<T extends string>(value: unknown, allowed: readonly T[], where: string): T {
  return typeof value === "string" && (allowed as readonly string[]).includes(value)
    ? (value as T)
    : reject(where, `expected one of ${allowed.join(", ")}`);
}
function object(value: unknown, where: string): Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : reject(where, "expected an object");
}
function parsePayload(row: SqlRow, where: string): Record<string, unknown> {
  return object(storedJson(row.payload, `${where}.payload`), `${where}.payload`);
}

/** A row to write: exact integers (bigint beyond 2^53) and BLOBs as read keep their type. */
export type NewRow = Record<string, string | number | bigint | null | { readonly blob: string }>;

/** The envelope columns every carried graph row shares (column-map-fleet.md, common table). */
function envelopeColumns(
  row: SqlRow,
  envelope: Record<string, unknown>,
  where: string,
  runTime: number,
): NewRow {
  const actor = object(envelope.actor, `${where}.actor`);
  const kind = oneOf(actor.kind, ["human", "session", "router"] as const, `${where}.actor.kind`);
  let actorColumns: NewRow;
  if (kind === "human") {
    actorColumns = {
      actor_kind: "human",
      actor_human_id: typed(actor.humanId, `${where}.actor.humanId`, humanId),
      actor_service_id: null,
      actor_endpoint_id: null,
      actor_session_id: null,
      actor_router_component: null,
    };
  } else if (kind === "session") {
    const session = object(actor.session, `${where}.actor.session`);
    const endpoint = object(session.endpoint, `${where}.actor.session.endpoint`);
    actorColumns = {
      actor_kind: "session",
      actor_human_id: null,
      actor_service_id: typed(endpoint.serviceId, `${where}.actor.serviceId`, serviceId),
      actor_endpoint_id: typed(endpoint.endpointId, `${where}.actor.endpointId`, endpointId),
      actor_session_id: typed(session.sessionId, `${where}.actor.sessionId`, sessionId),
      actor_router_component: null,
    };
  } else {
    const component = oneOf(
      actor.component,
      ["subscriptionDelivery", "archiveObservation"] as const,
      `${where}.actor.component`,
    );
    actorColumns = {
      actor_kind: "router",
      actor_human_id: null,
      actor_service_id: typed(actor.installation, `${where}.actor.installation`, serviceId),
      actor_endpoint_id: null,
      actor_session_id: null,
      actor_router_component:
        component === "subscriptionDelivery" ? "subscription_delivery" : "archive_observation",
    };
  }
  const placement = object(envelope.position, `${where}.position`);
  if (placement.kind !== "fleet") reject(`${where}.position`, "expected a Fleet position");
  const position = object(placement.position, `${where}.position.position`);
  const timeBasis = oneOf(
    row.time_basis,
    ["router", "legacy_unknown"] as const,
    `${where}.time_basis`,
  );
  const routerTime = optionalTyped(row.router_time, `${where}.router_time`, canonicalRouterTime);
  if ((timeBasis === "router") !== (routerTime !== null)) {
    reject(`${where}.router_time`, "router time must be present exactly when the basis is router");
  }
  const storedImportDigest = optionalTyped(
    row.import_digest,
    `${where}.import_digest`,
    importDigest,
  );
  if (timeBasis === "legacy_unknown" && storedImportDigest === null) {
    reject(`${where}.import_digest`, "a legacy-time row needs its import digest");
  }
  const request = envelope.request;
  return {
    ...actorColumns,
    request_id:
      request === null || request === undefined ? null : uuid(request, `${where}.request`),
    fleet_position: count(position.position, `${where}.position.position`),
    router_time: routerTime,
    time_basis: timeBasis,
    origin_installation: typed(row.origin_installation, `${where}.origin_installation`, serviceId),
    origin_machine: typed(row.origin_machine, `${where}.origin_machine`, machineLabel),
    import_digest: storedImportDigest,
    created_at: runTime,
    updated_at: runTime,
  };
}

function record(payload: Record<string, unknown>, kind: string, where: string) {
  if (payload.kind !== kind) reject(`${where}.payload.kind`, `expected ${kind}`);
  const body = object(payload.record, `${where}.payload.record`);
  return { body, envelope: object(body.envelope, `${where}.envelope`) };
}

function repositoryColumns(reference: Record<string, unknown>, where: string): NewRow {
  const kind = oneOf(reference.kind, ["origin", "local"] as const, `${where}.kind`);
  return kind === "origin"
    ? {
        reference_kind: "origin",
        normalized_origin: typed(
          reference.normalizedOrigin,
          `${where}.normalizedOrigin`,
          normalizedOrigin,
        ),
        service_id: null,
        common_directory: null,
      }
    : {
        reference_kind: "local",
        normalized_origin: null,
        service_id: typed(reference.serviceId, `${where}.serviceId`, serviceId),
        common_directory: typed(
          reference.commonDirectory,
          `${where}.commonDirectory`,
          commonDirectory,
        ),
      };
}

export interface CarriedFleetRows {
  readonly fleet_workspaces: NewRow[];
  readonly fleet_workspace_repository_refs: NewRow[];
  readonly fleet_boards: NewRow[];
  readonly fleet_project_directory: NewRow[];
  readonly fleet_workspace_adrs: NewRow[];
  readonly fleet_adr_repository_files: NewRow[];
  readonly fleet_repository_attachments: NewRow[];
}

export interface OldFleetGraph {
  readonly workspaces: readonly SqlRow[];
  readonly boards: readonly SqlRow[];
  readonly project_directory: readonly SqlRow[];
  readonly workspace_adrs: readonly SqlRow[];
  readonly repository_attachments: readonly SqlRow[];
}

/** Child rows get fresh UUIDv7 ids from `newId`; everything else keeps its value. */
export function convertFleetGraph(
  old: OldFleetGraph,
  runTime: number,
  newId: () => string,
): CarriedFleetRows {
  const rows: CarriedFleetRows = {
    fleet_workspaces: [],
    fleet_workspace_repository_refs: [],
    fleet_boards: [],
    fleet_project_directory: [],
    fleet_workspace_adrs: [],
    fleet_adr_repository_files: [],
    fleet_repository_attachments: [],
  };
  for (const row of old.workspaces) {
    const where = `workspaces/${String(row.workspace_id)}`;
    const { body, envelope } = record(parsePayload(row, where), "workspace", where);
    const workspaceId = uuid(row.workspace_id, `${where}.workspace_id`);
    if (envelope.id !== workspaceId) reject(where, "payload id differs from the row key");
    const repositories = Array.isArray(body.repositories)
      ? body.repositories
      : reject(`${where}.repositories`, "expected an array");
    if (repositories.length > 16) reject(`${where}.repositories`, "more than 16");
    rows.fleet_workspaces.push({
      workspace_id: workspaceId,
      name: typed(row.name, `${where}.name`, resourceName),
      description: typed(body.description, `${where}.description`, descriptionText),
      state: oneOf(body.state, ["active", "archiving", "archived"] as const, `${where}.state`),
      repository_count: repositories.length,
      ...envelopeColumns(row, envelope, where, runTime),
    });
    repositories.forEach((reference, ordinal) => {
      rows.fleet_workspace_repository_refs.push({
        row_id: newId(),
        workspace_id: workspaceId,
        ordinal,
        ...repositoryColumns(
          object(reference, `${where}.repositories[${ordinal}]`),
          `${where}.repositories[${ordinal}]`,
        ),
        created_at: runTime,
        updated_at: runTime,
      });
    });
  }
  for (const row of old.boards) {
    const where = `boards/${String(row.board_id)}`;
    const { body, envelope } = record(parsePayload(row, where), "board", where);
    const boardId = uuid(row.board_id, `${where}.board_id`);
    if (envelope.id !== boardId) reject(where, "payload id differs from the row key");
    const kind = object(body.kind, `${where}.kind`);
    const boardKind = oneOf(kind.kind, ["initiative", "area"] as const, `${where}.kind.kind`);
    rows.fleet_boards.push({
      board_id: boardId,
      workspace_id: uuid(row.workspace_id, `${where}.workspace_id`),
      name: typed(row.name, `${where}.name`, resourceName),
      description: typed(body.description, `${where}.description`, descriptionText),
      board_kind: boardKind,
      initiative_goal: boardKind === "initiative" ? text(kind.goal, `${where}.kind.goal`) : null,
      initiative_done_when:
        boardKind === "initiative" ? text(kind.doneWhen, `${where}.kind.doneWhen`) : null,
      state: oneOf(
        body.state,
        ["open", "closed", "archiving", "archived"] as const,
        `${where}.state`,
      ),
      ...envelopeColumns(row, envelope, where, runTime),
    });
  }
  for (const row of old.project_directory) {
    const where = `project_directory/${String(row.project_id)}`;
    // SQL labels this row "project"; the stored Record::DirectoryEntry tag differs.
    const { envelope } = record(parsePayload(row, where), "directoryEntry", where);
    const projectId = uuid(row.project_id, `${where}.project_id`);
    if (envelope.id !== projectId) reject(where, "payload id differs from the row key");
    rows.fleet_project_directory.push({
      project_id: projectId,
      workspace_id: uuid(row.workspace_id, `${where}.workspace_id`),
      board_id: uuid(row.board_id, `${where}.board_id`),
      name: typed(row.name, `${where}.name`, resourceName),
      ...envelopeColumns(row, envelope, where, runTime),
    });
  }
  // A superseded decision is inserted before the one superseding it (self-FK).
  const adrs = new Map(old.workspace_adrs.map((row) => [String(row.adr_id), row]));
  const ordered: SqlRow[] = [];
  const placed = new Set<string>();
  const place = (id: string, depth: number) => {
    if (placed.has(id)) return;
    if (depth > adrs.size) reject(`workspace_adrs/${id}`, "supersedes cycle");
    const row = adrs.get(id) ?? reject(`workspace_adrs/${id}`, "supersedes a missing decision");
    const body = object(object(parsePayload(row, `workspace_adrs/${id}`).record, id), id);
    if (typeof body.supersedes === "string") place(body.supersedes, depth + 1);
    placed.add(id);
    ordered.push(row);
  };
  for (const id of adrs.keys()) place(id, 0);
  for (const row of ordered) {
    const where = `workspace_adrs/${String(row.adr_id)}`;
    const { body, envelope } = record(parsePayload(row, where), "adr", where);
    const adrId = uuid(row.adr_id, `${where}.adr_id`);
    if (envelope.id !== adrId) reject(where, "payload id differs from the row key");
    const files = Array.isArray(body.repositoryFiles)
      ? body.repositoryFiles
      : reject(`${where}.repositoryFiles`, "expected an array");
    if (files.length > 16) reject(`${where}.repositoryFiles`, "more than 16");
    const madeIn =
      body.madeIn === null || body.madeIn === undefined
        ? null
        : object(body.madeIn, `${where}.madeIn`);
    rows.fleet_workspace_adrs.push({
      adr_id: adrId,
      workspace_id: uuid(row.workspace_id, `${where}.workspace_id`),
      title: typed(row.title, `${where}.title`, resourceName),
      text: typed(body.text, `${where}.text`, markdownText),
      status: oneOf(
        body.status,
        ["proposed", "accepted", "superseded", "withdrawn"] as const,
        `${where}.status`,
      ),
      supersedes_adr_id:
        body.supersedes === null || body.supersedes === undefined
          ? null
          : uuid(body.supersedes, `${where}.supersedes`),
      made_in_kind:
        madeIn === null ? null : oneOf(madeIn.kind, RECORD_KINDS, `${where}.madeIn.kind`),
      made_in_record_id: madeIn === null ? null : uuid(madeIn.id, `${where}.madeIn.id`),
      revision: String(count(body.revision, `${where}.revision`)),
      repository_file_count: files.length,
      ...envelopeColumns(row, envelope, where, runTime),
    });
    files.forEach((file, ordinal) => {
      const at = `${where}.repositoryFiles[${ordinal}]`;
      const artifact = object(file, at);
      rows.fleet_adr_repository_files.push({
        id: newId(),
        adr_id: adrId,
        ordinal,
        repository_origin: typed(artifact.repository, `${at}.repository`, normalizedOrigin),
        path: text(artifact.path, `${at}.path`),
        commit_sha: optionalTyped(artifact.commit, `${at}.commit`, commitSha),
        created_at: runTime,
        updated_at: runTime,
      });
    });
  }
  for (const row of old.repository_attachments) {
    const where = `repository_attachments/${String(row.attachment_id)}`;
    const reference = storedJson(row.repository_reference, `${where}.repository_reference`);
    const timeBasis = oneOf(
      row.time_basis,
      ["router", "legacy_unknown"] as const,
      `${where}.time_basis`,
    );
    rows.fleet_repository_attachments.push({
      attachment_id: uuid(row.attachment_id, `${where}.attachment_id`),
      workspace_id: uuid(row.workspace_id, `${where}.workspace_id`),
      ...repositoryColumns(object(reference, `${where}.repository_reference`), where),
      router_time: optionalTyped(row.router_time, `${where}.router_time`, canonicalRouterTime),
      time_basis: timeBasis,
      origin_installation: typed(
        row.origin_installation,
        `${where}.origin_installation`,
        serviceId,
      ),
      origin_machine: typed(row.origin_machine, `${where}.origin_machine`, machineLabel),
      import_digest: optionalTyped(row.import_digest, `${where}.import_digest`, importDigest),
      created_at: runTime,
      updated_at: runTime,
    });
  }
  return rows;
}

/**
 * `INSERT INTO table(columns) VALUES(?, …)` for one row, in the row's key order.
 * A bigint binds as its decimal text through CAST(? AS INTEGER), since a Durable
 * Object binding takes no bigint; a BLOB binds as hex through unhex(?).
 */
export function insertStatement(table: string, row: NewRow) {
  const columns = Object.keys(row);
  const placeholders = columns.map((column) => {
    const value = row[column];
    return typeof value === "bigint"
      ? "CAST(? AS INTEGER)"
      : typeof value === "object" && value !== null
        ? "unhex(?)"
        : "?";
  });
  return {
    sql: `INSERT INTO ${table}(${columns.join(",")}) VALUES(${placeholders.join(",")})`,
    bindings: columns.map((column) => {
      const value = row[column] ?? null;
      return typeof value === "bigint"
        ? value.toString()
        : typeof value === "object" && value !== null
          ? value.blob
          : value;
    }),
  };
}
