// Test only: production-shaped storage for the migration, seeded through the
// old-central stand-in with production's verbatim DDL. Two Macs hold two
// projects (one at lease 3, after holder changes), with one workspace, board,
// two directory entries, two decisions (one superseding the other) and a
// repository attachment. Each scenario may alter the seed to set up one
// contradiction.

import { createHash } from "node:crypto";
import { parseExact } from "../../src/exact_json.ts";
import { SCHEMA_SOURCES } from "../../src/generated/schema_sources.ts";
import { RECEIVER, type startWorker } from "./workerd_harness.ts";

type Runtime = Awaited<ReturnType<typeof startWorker>>;

export const IDS = {
  machineA: "01a1c070-0000-7000-8000-00000000a001",
  machineB: "01a1c070-0000-7000-8000-00000000b001",
  serviceA: "01a1c070-0000-7000-8000-00000000a002",
  serviceB: "01a1c070-0000-7000-8000-00000000b002",
  projectHeld3: "01a1c070-0000-7000-8000-000000000001",
  projectHeld1: "01a1c070-0000-7000-8000-000000000002",
  projectRetired: "01a1c070-0000-7000-8000-000000000003",
  workspace: "01a1c070-0000-7000-8000-0000000000e1",
  board: "01a1c070-0000-7000-8000-0000000000e2",
  adrOld: "01a1c070-0000-7000-8000-0000000000e3",
  adrNew: "01a1c070-0000-7000-8000-0000000000e4",
  attachment: "01a1c070-0000-7000-8000-0000000000e5",
  feedState: "01a1c070-0000-7000-8000-0000000000e6",
  operation: "01a1c070-0000-7000-8000-0000000000e7",
  registration: "01a1c070-0000-7000-8000-0000000000d1",
  renewal: "01a1c070-0000-7000-8000-0000000000d2",
  move: "01a1c070-0000-7000-8000-0000000000d3",
  moveOperation: "01a1c070-0000-7000-8000-0000000000d4",
  cancelledMove: "01a1c070-0000-7000-8000-0000000000d5",
  cancelledOperation: "01a1c070-0000-7000-8000-0000000000d6",
  preview: "01a1c070-0000-7000-8000-0000000000d7",
  session: "01a1c070-0000-7000-8000-0000000000d8",
} as const;
export const PROJECTS = [IDS.projectHeld3, IDS.projectHeld1, IDS.projectRetired] as const;

const split = (sql: string): string[] =>
  sql
    .split(/;\s*\n/)
    .map((statement) => statement.trim())
    .filter((statement) => statement !== "");

const quote = (value: string): string => `'${value.replaceAll("'", "''")}'`;
/** Production's checksum text for a migration: a JSON array of its SHA-384 bytes. */
const productionChecksum = (sql: string): string =>
  JSON.stringify([...createHash("sha384").update(sql).digest()]);
let rowCounter = 0;
const rowId = (): string =>
  `01a1c070-0000-7000-8000-${(0x100000 + (rowCounter += 1)).toString(16).padStart(12, "0")}`;
const historyRow = (corpus: string, version: number, checksum: string) =>
  `INSERT INTO central_schema_migrations VALUES ('${rowId()}','${corpus}','${version}',${quote(checksum)})`;

const ROUTER_TIME = "2026-11-01T12:00:00+00:00";
const envelope = (id: string, position: number) => ({
  id,
  scope: { workspace: IDS.workspace, board: null, project: null },
  actor: { kind: "human", humanId: "shravan" },
  request: null,
  position: {
    kind: "fleet",
    position: { position, origin: { installation: IDS.serviceA, machine: "Sunbook" } },
  },
  time: { kind: "router", value: "2026-11-01T12:00:00Z" },
});
const graphColumns = `${quote(ROUTER_TIME)},'router',${quote(IDS.serviceA)},'Sunbook',NULL`;

/** The statements each object's seed runs; a scenario edits them before seeding. */
export interface ProductionSeed {
  registry: string[];
  fleet: string[];
  copies: Record<string, string[]>;
  /** Retiring classes, keyed by Router installation (O object_location.rs:6,8). */
  connections: Record<string, string[]>;
  mailboxes: Record<string, string[]>;
  /** S2's facts from each holder Mac's retained file, as migrate's request carries them. */
  macFiles: {
    database: string;
    holder: string;
    lease: number | bigint;
    senderReceiver: string;
    retained: {
      floor: { epoch: number; sequence: number };
      head: { epoch: number; sequence: number };
    };
  }[];
}

/**
 * One valid row in every other lease table, as the old library wrote them
 * (serde JSON of registry_records.rs and fleet-sync-protocol): the held-at-one
 * project's registration and a renewal, the held-at-three project's completed
 * move from lease 2 to Sunclaw, a cancelled move with its cancel record, a
 * takeover preview, and a schema-support report.
 */
function leaseHistory(): string[] {
  const json = (value: unknown) => quote(JSON.stringify(value));
  const leaseStatus = {
    database: IDS.projectHeld1,
    epoch: 1,
    holder: IDS.machineA,
    state: "held",
    expiresAt: 1_794_240_000_000,
    remainingMs: 14_400_000,
  };
  const moveHistory = [
    { phase: { phase: "requested" }, enteredAt: 1_794_000_000_000 },
    { phase: { phase: "releasing" }, enteredAt: 1_794_000_001_000 },
    {
      phase: { phase: "released", finalPosition: { epoch: 2, sequence: 20 } },
      enteredAt: 1_794_000_002_000,
    },
    { phase: { phase: "fencing" }, enteredAt: 1_794_000_003_000 },
    { phase: { phase: "ended" }, enteredAt: 1_794_000_004_000 },
  ];
  const cancelledHistory = [
    { phase: { phase: "requested" }, enteredAt: 1_794_100_000_000 },
    { phase: { phase: "cancelling" }, enteredAt: 1_794_100_001_000 },
    { phase: { phase: "ended" }, enteredAt: 1_794_100_002_000 },
  ];
  return [
    `INSERT INTO _fleet_sync_lifecycle_attempts VALUES (${quote(IDS.registration)},${quote(IDS.projectHeld1)},${json({ kind: "register", request: { attempt: IDS.registration, database: IDS.projectHeld1, writer: IDS.machineA, origin: { kind: "empty" } } })},${json({ kind: "registration", answer: { kind: "registered", value: { lease: leaseStatus, replayed: false } } })})`,
    `INSERT INTO _fleet_sync_renewal_attempts VALUES (${quote(IDS.projectHeld1)},${quote(IDS.renewal)},${json({ request: { attempt: IDS.renewal, database: IDS.projectHeld1, epoch: 1 }, holder: IDS.machineA, answer: { kind: "renewed", value: { recorded: { epoch: 1, expiresAt: 1_794_240_000_000 }, current: leaseStatus, transferInProgress: null, replayed: false } } })})`,
    `INSERT INTO _fleet_sync_transfer_admissions(attempt,operation,request,requested_by,admitted_at,follow_up,target_schema) VALUES (${quote(IDS.move)},${json(IDS.moveOperation)},${json({ kind: "move", value: { attempt: IDS.move, database: IDS.projectHeld3, fromEpoch: 2, to: IDS.machineB, reason: "moving to Sunclaw" } })},${json("shravan")},'1794000000000',NULL,'null')`,
    `INSERT INTO _fleet_sync_transfers(attempt,database,intent,outcome,phase,phase_history,cleanup_failed) VALUES (${quote(IDS.move)},${quote(IDS.projectHeld3)},'3',${json({ kind: "granted", value: { epoch: 3, expiresAt: 1_794_014_400_000, start: { epoch: 2, sequence: 20 } } })},${json({ phase: "ended" })},${json(moveHistory)},NULL)`,
    `INSERT INTO _fleet_sync_transfer_admissions(attempt,operation,request,requested_by,admitted_at,follow_up,target_schema) VALUES (${quote(IDS.cancelledMove)},${json(IDS.cancelledOperation)},${json({ kind: "move", value: { attempt: IDS.cancelledMove, database: IDS.projectHeld1, fromEpoch: 1, to: IDS.machineB, reason: "trying Sunclaw" } })},${json("shravan")},'1794100000000',NULL,${json({ maxVersion: 1 })})`,
    `INSERT INTO _fleet_sync_transfers(attempt,database,intent,outcome,phase,phase_history,cleanup_failed) VALUES (${quote(IDS.cancelledMove)},${quote(IDS.projectHeld1)},'null',${json({ kind: "cancelled", value: { by: "shravan" } })},${json({ phase: "ended" })},${json(cancelledHistory)},NULL)`,
    `INSERT INTO _fleet_sync_transfer_cancels VALUES (${quote(IDS.cancelledMove)},${json({ by: "shravan", reason: "changed my mind", at: 1_794_100_001_000 })})`,
    `INSERT INTO _fleet_sync_takeover_previews VALUES (${quote(IDS.preview)},${quote(IDS.projectHeld3)},${json({ database: IDS.projectHeld3, preview: { token: IDS.preview, fence: 2, acked: { epoch: 2, sequence: 20 }, holder: IDS.machineA, lastSeen: 1_793_999_990_000 } })})`,
    `INSERT INTO _fleet_sync_schema_support VALUES (${quote(IDS.machineA)},${json({ peer: IDS.machineA, sessionId: IDS.session, attachSeq: 4, support: { maxVersion: 1 } })})`,
  ];
}

export function productionSeed(): ProductionSeed {
  // Production's DDL, verbatim (src/generated/schema_sources.ts, from git at 2636806a5).
  const history = SCHEMA_SOURCES.production.history.flatMap(split);
  const journal = split(SCHEMA_SOURCES.production.journal);
  const lease = SCHEMA_SOURCES.production.lease.flatMap(split);
  const registrySources = SCHEMA_SOURCES.production.registry;
  const registryDdl = registrySources.flatMap(split);
  // production.fleet[0] is the 0000 history DDL; [1..] are central-fleet 1–10.
  const fleetDdl = SCHEMA_SOURCES.production.fleet.flatMap(split);
  const fleetSources = SCHEMA_SOURCES.production.fleet.slice(1);

  const leaseRecord = (database: string, holder: string, epoch: number) =>
    JSON.stringify({
      database,
      holder,
      epoch,
      expires_at: 1_794_240_000_000,
      last_renewed_at: 1_794_225_600_000,
    });
  const databaseRecord = (database: string, epoch: number) =>
    JSON.stringify({
      database,
      lifecycle: { state: "registered", origin: { kind: "empty" }, importedFrom: null },
      revision: { epoch, change: 1 },
      changed_at: 1_794_225_600_000,
    });
  const registry = [
    ...history,
    ...lease,
    ...registryDdl,
    ...journal,
    ...SCHEMA_SOURCES.production.lease.map((sql, index) =>
      historyRow("fleet-sync-lease", index + 1, productionChecksum(sql)),
    ),
    ...registrySources.map((sql, index) =>
      historyRow("central-registry", index + 1, productionChecksum(sql)),
    ),
    `INSERT INTO machine_enrollments VALUES (${quote(IDS.machineA)},${quote(IDS.serviceA)},'Sunbook','client-a','hash-a','enrolled','2027-01-01T00:00:00+00:00','2026-11-09T11:59:00.123Z',NULL,${quote(IDS.operation)})`,
    `INSERT INTO machine_enrollments VALUES (${quote(IDS.machineB)},${quote(IDS.serviceB)},'Sunclaw','client-b','hash-b','enrolled','2027-01-01T00:00:00+00:00',NULL,NULL,NULL)`,
    `INSERT INTO _fleet_sync_databases VALUES (${quote(IDS.projectHeld3)},${quote(databaseRecord(IDS.projectHeld3, 3))})`,
    `INSERT INTO _fleet_sync_databases VALUES (${quote(IDS.projectHeld1)},${quote(databaseRecord(IDS.projectHeld1, 1))})`,
    `INSERT INTO _fleet_sync_leases VALUES (${quote(IDS.projectHeld3)},${quote(leaseRecord(IDS.projectHeld3, IDS.machineB, 3))})`,
    `INSERT INTO _fleet_sync_leases VALUES (${quote(IDS.projectHeld1)},${quote(leaseRecord(IDS.projectHeld1, IDS.machineA, 1))})`,
    `INSERT INTO _fleet_sync_epoch_floor VALUES (${quote(IDS.projectHeld3)},'3')`,
    `INSERT INTO _fleet_sync_epoch_floor VALUES (${quote(IDS.projectHeld1)},'1')`,
    `INSERT INTO _fleet_sync_databases VALUES (${quote(IDS.projectRetired)},${quote(JSON.stringify({ database: IDS.projectRetired, lifecycle: { state: "retired", at: 1_794_100_000_000 }, revision: { epoch: 2, change: 3 }, changed_at: 1_794_100_000_000 }))})`,
    `INSERT INTO _fleet_sync_epoch_floor VALUES (${quote(IDS.projectRetired)},'2')`,
    ...[1, 2, 3].map(
      (epoch) =>
        `INSERT INTO _fleet_sync_epoch_history VALUES (${quote(IDS.projectHeld3)},'${epoch}',${quote(JSON.stringify({ database: IDS.projectHeld3, epoch, holder: epoch === 2 ? IDS.machineA : IDS.machineB, granted_at: 1_793_923_200_000 + epoch * 86_400_000, ended_at: epoch === 3 ? null : { epoch, sequence: 10 * epoch } }))})`,
    ),
    `INSERT INTO registry_lease_incarnation VALUES ('01a1c070-0000-7000-8000-0000000000f2','registry','7')`,
    ...leaseHistory(),
  ];

  const workspace = {
    kind: "workspace",
    record: {
      envelope: envelope(IDS.workspace, 1),
      name: "agent collaboration",
      description: "",
      state: "active",
      repositories: [
        { kind: "origin", normalizedOrigin: "github.com/agent-collaboration/agent-collaboration" },
      ],
    },
  };
  const board = {
    kind: "board",
    record: {
      envelope: envelope(IDS.board, 2),
      workspace: IDS.workspace,
      name: "central",
      description: "D1",
      kind: { kind: "initiative", goal: "central in place", doneWhen: "every project syncs" },
      state: "open",
    },
  };
  const project = (id: string, title: string, position: number) => ({
    kind: "project",
    record: { envelope: envelope(id, position), board: IDS.board, title },
  });
  const adr = (id: string, supersedes: string | null, status: string, position: number) => ({
    kind: "adr",
    record: {
      envelope: envelope(id, position),
      workspace: IDS.workspace,
      title: `decision ${position}`,
      text: "one central store",
      status,
      supersedes,
      madeIn: null,
      revision: 1,
      repositoryFiles: [
        {
          repository: "github.com/agent-collaboration/agent-collaboration",
          path: "docs/d1.md",
          commit: null,
        },
      ],
    },
  });
  const fleet = [
    ...fleetDdl,
    ...journal,
    ...fleetSources.map((sql, index) =>
      historyRow("central-fleet", index + 1, productionChecksum(sql)),
    ),
    `INSERT INTO workspaces VALUES (${quote(IDS.workspace)},'agent collaboration',${quote(JSON.stringify(workspace))},${graphColumns})`,
    `INSERT INTO boards(board_id,workspace_id,name,payload,router_time,time_basis,origin_installation,origin_machine,import_digest) VALUES (${quote(IDS.board)},${quote(IDS.workspace)},'central',${quote(JSON.stringify(board))},${graphColumns})`,
    ...[
      [IDS.projectHeld3, "held at three", 3],
      [IDS.projectHeld1, "held at one", 4],
    ].map(
      ([id, title, position]) =>
        `INSERT INTO project_directory(project_id,workspace_id,board_id,name,payload,router_time,time_basis,origin_installation,origin_machine,import_digest) VALUES (${quote(String(id))},${quote(IDS.workspace)},${quote(IDS.board)},${quote(String(title))},${quote(JSON.stringify(project(String(id), String(title), Number(position))))},${graphColumns})`,
    ),
    // The superseding decision is written first, so the conversion must order them.
    `INSERT INTO workspace_adrs(adr_id,workspace_id,title,payload,router_time,time_basis,origin_installation,origin_machine,import_digest) VALUES (${quote(IDS.adrNew)},${quote(IDS.workspace)},'decision 6',${quote(JSON.stringify(adr(IDS.adrNew, IDS.adrOld, "accepted", 6)))},${graphColumns})`,
    `INSERT INTO workspace_adrs(adr_id,workspace_id,title,payload,router_time,time_basis,origin_installation,origin_machine,import_digest) VALUES (${quote(IDS.adrOld)},${quote(IDS.workspace)},'decision 5',${quote(JSON.stringify(adr(IDS.adrOld, null, "superseded", 5)))},${graphColumns})`,
    `INSERT INTO repository_attachments(attachment_id,workspace_id,repository_reference,router_time,time_basis,origin_installation,origin_machine,import_digest) VALUES (${quote(IDS.attachment)},${quote(IDS.workspace)},'{"kind":"local","serviceId":"${IDS.serviceA}","commonDirectory":"/code/repo"}','2026-11-01T12:00:00Z','router',${quote(IDS.serviceA)},'Sunbook',NULL)`,
    `INSERT INTO fleet_feed_state VALUES (${quote(IDS.feedState)},'fleet',42,0)`,
    `INSERT INTO lease_incarnation_fence VALUES ('01a1c070-0000-7000-8000-0000000000f3','registry','9')`,
  ];

  const receiver = SCHEMA_SOURCES.production.receiver;
  const copy = (database: string, holder: string, epoch: number) => [
    ...history,
    ...receiver.flatMap(split),
    ...journal,
    ...receiver.map((sql, index) =>
      historyRow("fleet-sync-receiver", index + 1, productionChecksum(sql)),
    ),
    `INSERT INTO _fleet_sync_copy VALUES (1,${quote(JSON.stringify({ state: "following", copy: { database, role: "authoritative", fence: epoch, holder, start: { epoch: 1, sequence: 0 }, acked: { epoch, sequence: 17 }, schema: { version: 1, fingerprint: "schema-fingerprint" } } }))})`,
    "CREATE TABLE project_tasks(id TEXT PRIMARY KEY, title TEXT NOT NULL)",
    `INSERT INTO project_tasks VALUES ('task-1','keep me')`,
  ];
  const connection = (service: string) => [
    ...history,
    ...SCHEMA_SOURCES.production.connection.flatMap(split),
    ...journal,
    `INSERT INTO machine_notice_state VALUES ('01a1c070-0000-7000-8000-0000000000fb','notices',5,2,${quote(service === IDS.serviceA ? IDS.machineA : IDS.machineB)})`,
  ];
  const mailbox = () => [
    ...history,
    ...SCHEMA_SOURCES.production.mailbox.flatMap(split),
    ...journal,
    `INSERT INTO _fleet_sync_mail_items(item_id,stream,seq,live,capacity_slot,payload_bytes,settlement_bytes,record) VALUES ('01a1c070-0000-7000-8000-0000000000fc','stream',1,1,1,10,0,'{"kind":"mail"}')`,
    `INSERT INTO _fleet_sync_operations(operation_id,kind,kind_version,request_key,input,input_digest,state,attempt) VALUES ('01a1c070-0000-7000-8000-0000000000fd','deliver',1,'k',x'0102',x'abcd','completed',1)`,
  ];
  return {
    registry,
    fleet,
    connections: {
      [IDS.serviceA]: connection(IDS.serviceA),
      [IDS.serviceB]: connection(IDS.serviceB),
    },
    mailboxes: { [IDS.serviceA]: mailbox(), [IDS.serviceB]: mailbox() },
    macFiles: [
      {
        database: IDS.projectHeld3,
        holder: IDS.machineB,
        lease: 3,
        senderReceiver: RECEIVER,
        // A transferred writer at lease 3 whose unacknowledged tail runs past the copy's 3/17.
        retained: { floor: { epoch: 1, sequence: 0 }, head: { epoch: 3, sequence: 20 } },
      },
      {
        database: IDS.projectHeld1,
        holder: IDS.machineA,
        lease: 1,
        senderReceiver: RECEIVER,
        retained: { floor: { epoch: 1, sequence: 0 }, head: { epoch: 1, sequence: 17 } },
      },
    ],
    copies: {
      [IDS.projectHeld3]: copy(IDS.projectHeld3, IDS.machineB, 3),
      [IDS.projectHeld1]: copy(IDS.projectHeld1, IDS.machineA, 1),
      [IDS.projectRetired]: [
        ...copy(IDS.projectRetired, IDS.machineA, 2),
        `UPDATE _fleet_sync_copy SET record = ${quote(JSON.stringify({ state: "retired", copy: { database: IDS.projectRetired, retired_at: 1_794_100_000_000 } }))}`,
      ],
    },
  };
}

export const run = async (
  runtime: Runtime,
  binding: string,
  name: string,
  statements: string[],
) => {
  const response = await runtime.dispatchFetch(
    `https://central.test/${binding}/${encodeURIComponent(name)}/run`,
    { method: "POST", body: JSON.stringify(statements) },
  );
  if (response.status !== 200) throw new Error(`seed ${binding}/${name}: ${await response.text()}`);
};

export async function seed(runtime: Runtime, production: ProductionSeed) {
  await run(runtime, "LEASE_REGISTRY", "registry", production.registry);
  await run(runtime, "FLEET", "fleet", production.fleet);
  for (const [project, statements] of Object.entries(production.copies)) {
    await run(runtime, "PROJECT_COPIES", project, statements);
  }
  for (const [service, statements] of Object.entries(production.connections)) {
    await run(runtime, "CONNECTIONS", service, statements);
  }
  for (const [service, statements] of Object.entries(production.mailboxes)) {
    await run(runtime, "MAILBOXES", service, statements);
  }
}

/** Every object's schema and rows, so a test can compare storage before and after. */
export async function snapshot(runtime: Runtime, projects: readonly string[] = PROJECTS) {
  const objects: Record<string, unknown> = {};
  for (const [binding, name] of [
    ["LEASE_REGISTRY", "registry"],
    ["FLEET", "fleet"],
    ...projects.map((project) => ["PROJECT_COPIES", project]),
    ...[IDS.serviceA, IDS.serviceB].flatMap((service) => [
      ["CONNECTIONS", service],
      ["MAILBOXES", service],
    ]),
  ] as const) {
    const response = await runtime.dispatchFetch(
      `https://central.test/${binding}/${encodeURIComponent(name)}/dump`,
    );
    objects[`${binding}/${name}`] = parseExact(await response.text());
  }
  return objects;
}
