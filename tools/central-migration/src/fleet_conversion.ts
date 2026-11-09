// Production's Fleet graph rows → main's typed rows (PD4), following the
// column map in tmp/2026-11-09-migration/column-map-fleet.md. Pure: every
// value is checked before any write, and a value main's types reject stops the
// run with the row named (R7) instead of being rewritten.

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
function optionalText(value: unknown, where: string): string | null {
  return value === undefined || value === null ? null : text(value, where);
}
function uuid(value: unknown, where: string): string {
  return isUuidV7(value) ? value : reject(where, "expected a canonical UUIDv7");
}
function count(value: unknown, where: string): number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    ? value
    : reject(where, "expected a nonnegative integer");
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
  let value: unknown;
  try {
    value = JSON.parse(text(row.payload, `${where}.payload`));
  } catch (error) {
    if (error instanceof Rejected) throw error;
    return reject(`${where}.payload`, "not JSON");
  }
  return object(value, `${where}.payload`);
}

export type NewRow = Record<string, string | number | null>;

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
      actor_human_id: text(actor.humanId, `${where}.actor.humanId`),
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
      actor_service_id: text(endpoint.serviceId, `${where}.actor.serviceId`),
      actor_endpoint_id: text(endpoint.endpointId, `${where}.actor.endpointId`),
      actor_session_id: text(session.sessionId, `${where}.actor.sessionId`),
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
      actor_service_id: text(actor.installation, `${where}.actor.installation`),
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
  const routerTime = optionalText(row.router_time, `${where}.router_time`);
  if ((timeBasis === "router") !== (routerTime !== null)) {
    reject(`${where}.router_time`, "router time must be present exactly when the basis is router");
  }
  const importDigest = optionalText(row.import_digest, `${where}.import_digest`);
  if (timeBasis === "legacy_unknown" && importDigest === null) {
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
    origin_installation: text(row.origin_installation, `${where}.origin_installation`),
    origin_machine: text(row.origin_machine, `${where}.origin_machine`),
    import_digest: importDigest,
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
        normalized_origin: text(reference.normalizedOrigin, `${where}.normalizedOrigin`),
        service_id: null,
        common_directory: null,
      }
    : {
        reference_kind: "local",
        normalized_origin: null,
        service_id: text(reference.serviceId, `${where}.serviceId`),
        common_directory: text(reference.commonDirectory, `${where}.commonDirectory`),
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
      name: text(row.name, `${where}.name`),
      description: text(body.description, `${where}.description`),
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
      name: text(row.name, `${where}.name`),
      description: text(body.description, `${where}.description`),
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
    const { envelope } = record(parsePayload(row, where), "project", where);
    const projectId = uuid(row.project_id, `${where}.project_id`);
    if (envelope.id !== projectId) reject(where, "payload id differs from the row key");
    rows.fleet_project_directory.push({
      project_id: projectId,
      workspace_id: uuid(row.workspace_id, `${where}.workspace_id`),
      board_id: uuid(row.board_id, `${where}.board_id`),
      name: text(row.name, `${where}.name`),
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
      title: text(row.title, `${where}.title`),
      text: text(body.text, `${where}.text`),
      status: oneOf(
        body.status,
        ["proposed", "accepted", "superseded", "withdrawn"] as const,
        `${where}.status`,
      ),
      supersedes_adr_id:
        body.supersedes === null || body.supersedes === undefined
          ? null
          : uuid(body.supersedes, `${where}.supersedes`),
      made_in_kind: madeIn === null ? null : text(madeIn.kind, `${where}.madeIn.kind`),
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
        repository_origin: text(artifact.repository, `${at}.repository`),
        path: text(artifact.path, `${at}.path`),
        commit_sha: optionalText(artifact.commit, `${at}.commit`),
        created_at: runTime,
        updated_at: runTime,
      });
    });
  }
  for (const row of old.repository_attachments) {
    const where = `repository_attachments/${String(row.attachment_id)}`;
    let reference: unknown;
    try {
      reference = JSON.parse(text(row.repository_reference, `${where}.repository_reference`));
    } catch (error) {
      if (error instanceof Rejected) throw error;
      reject(`${where}.repository_reference`, "not JSON");
    }
    const timeBasis = oneOf(
      row.time_basis,
      ["router", "legacy_unknown"] as const,
      `${where}.time_basis`,
    );
    rows.fleet_repository_attachments.push({
      attachment_id: uuid(row.attachment_id, `${where}.attachment_id`),
      workspace_id: uuid(row.workspace_id, `${where}.workspace_id`),
      ...repositoryColumns(object(reference, `${where}.repository_reference`), where),
      router_time: optionalText(row.router_time, `${where}.router_time`),
      time_basis: timeBasis,
      origin_installation: text(row.origin_installation, `${where}.origin_installation`),
      origin_machine: text(row.origin_machine, `${where}.origin_machine`),
      import_digest: optionalText(row.import_digest, `${where}.import_digest`),
      created_at: runTime,
      updated_at: runTime,
    });
  }
  return rows;
}

/** `INSERT INTO table(columns) VALUES(?, …)` for one row, in the row's key order. */
export function insertStatement(table: string, row: NewRow) {
  const columns = Object.keys(row);
  return {
    sql: `INSERT INTO ${table}(${columns.join(",")}) VALUES(${columns.map(() => "?").join(",")})`,
    bindings: columns.map((column) => row[column] ?? null),
  };
}
