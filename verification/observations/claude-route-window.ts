import { createHash } from "node:crypto";
import {
  closeSync,
  constants,
  fstatSync,
  lstatSync,
  openSync,
  readFileSync,
  readSync,
  realpathSync,
} from "node:fs";
import { homedir } from "node:os";
import { isAbsolute, join } from "node:path";
import { z } from "zod";

/**
 * The bounded Claude route window over the normal debug Host's existing Router audit file,
 * `<run_directory>/router-audit.jsonl` (automation_debug_host/launch_options.rs:257-273). The
 * runner records a baseline right before the submission Enter; the observer reads only the
 * segment appended after it, once, after the same-session reply readback. This module is the one
 * owner of the file binding, identity, digest and row rules both sides use.
 *
 * The audit has no timestamp, Session or prompt field and a constant request ID
 * (codex-router-core/src/audit.rs:83-98; http_affinity.rs:128-143), so attribution rests on one
 * isolated Host and Session window, never on a prompt join. A forwarded row is written when the
 * response head is forwarded, for any upstream status (claude_edge/server_pipeline.rs:229-240): it
 * proves neither HTTP success nor stream completion.
 */

const AUDIT_FILE = "router-audit.jsonl";
const HOST_MARKER = "debug-host-context.json";
// automation-debug-host.rs:315-346: the marker is a small owner-private regular file.
const MAX_MARKER_BYTES = 4096;
const READ_CHUNK_BYTES = 65_536;
// The finite bounds of the hashed prefix and the read segment, shared by the run plan and the
// baseline record so both entry points keep the same read limit.
export const MAX_ROUTE_PREFIX_BYTES = 67_108_864;
export const MAX_ROUTE_SEGMENT_BYTES = 1_048_576;

/** The declared normal Host whose Router writes the audit file, and the window's byte bounds. */
export interface ClaudeRouteBinding {
  readonly hostRunDirectory: string;
  readonly hostPid: number;
  readonly serviceDirectory: string;
  readonly maxPrefixBytes: number;
  readonly maxSegmentBytes: number;
}

// The Host's own context marker (automation-debug-host.rs:32-47, serde camelCase), written once
// at startup before its runtime runs.
const hostMarkerSchema = z.looseObject({
  kind: z.literal("debugHostPrepared"),
  runDirectory: z.string(),
  serviceDirectory: z.string(),
  port: z.number().int().positive(),
  hostPid: z.number().int().positive(),
  claudeProvider: z.looseObject({ executable: z.string() }).nullable(),
});
// The service manifest's published Router listener, `<host>:<port>` (claude_launch_target.rs:538-542).
const serviceListenerSchema = z.looseObject({ routerProxyEndpoint: z.string() });

/** What the runner recorded right before the submission Enter. */
export const claudeRouteBaselineSchema = z.strictObject({
  kind: z.literal("claude-route-baseline"),
  // The composed-input checkpoint whose original capture preceded this record.
  after_scene: z.enum(["prompt-composed", "skill-input-composed"]),
  host_run_directory: z.string().refine(isAbsolute, "absolute path required"),
  audit_file: z.string().refine(isAbsolute, "absolute path required"),
  host_pid: z.number().int().positive(),
  service_directory: z.string().refine(isAbsolute, "absolute path required"),
  router_port: z.number().int().positive(),
  provider_session_id: z.string().min(1),
  native_thread_id: z.uuid(),
  max_prefix_bytes: z.number().int().positive().max(MAX_ROUTE_PREFIX_BYTES),
  max_segment_bytes: z.number().int().positive().max(MAX_ROUTE_SEGMENT_BYTES),
  // The Router creates the file on its first decision (audit.rs:184-198), so it may not exist yet.
  file: z.discriminatedUnion("state", [
    z.strictObject({ state: z.literal("absent") }),
    z.strictObject({
      state: z.literal("present"),
      device: z.string().regex(/^\d+$/u),
      inode: z.string().regex(/^\d+$/u),
      byte_length: z.number().int().nonnegative(),
      prefix_sha256: z.string().regex(/^[a-f0-9]{64}$/u),
    }),
  ]),
});
export type ClaudeRouteBaseline = z.infer<typeof claudeRouteBaselineSchema>;

// One serialized AuditEvent (audit.rs:15-98, serde snake_case; RequestId is its bare string).
// Reasons and classes are static snake_case tokens; anything else is malformed, not retained text.
const tokenSchema = z.string().regex(/^[a-z][a-z_]{0,63}$/u);
const auditRowSchema = z.strictObject({
  event_type: z.literal("proxy_decision"),
  request_id: z.string(),
  route_kind: z.enum([
    "responses",
    "claude_messages",
    "models",
    "compact",
    "memory_trace",
    "responses_web_socket",
    "image_generations",
    "image_edits",
  ]),
  transport_kind: z.enum(["http", "web_socket"]),
  local_auth_result: z.enum(["valid", "missing", "empty", "old", "wrong"]),
  outcome: z.enum(["rejected", "allowed"]),
  decision_reason: tokenSchema,
  response_commit_state: z.enum(["not_committed", "committed"]),
  account_hash: z.string().min(1).optional(),
  error_class: tokenSchema.optional(),
});
type AuditRow = z.infer<typeof auditRowSchema>;

/** One categorical row class and its count; never a request ID, account hash or free text. */
export interface AuditCategory {
  readonly route_kind: AuditRow["route_kind"];
  readonly transport_kind: AuditRow["transport_kind"];
  readonly local_auth_result: AuditRow["local_auth_result"];
  readonly outcome: AuditRow["outcome"];
  readonly decision_reason: string;
  readonly response_commit_state: AuditRow["response_commit_state"];
  readonly error_class: string | null;
  readonly account_hash_present: boolean;
  readonly count: number;
}
export interface ClaudeRouteWindow {
  readonly baseline_state: "absent" | "present";
  readonly segment_bytes: number;
  readonly segment_sha256: string;
  readonly rows: number;
  readonly categories: readonly AuditCategory[];
}

function ownerPrivate(status: { mode: number; uid: number }): boolean {
  return status.uid === process.getuid?.() && (status.mode & 0o077) === 0;
}

/**
 * The audit file of exactly the declared Host: its owner-private run directory, whose own marker
 * names that run directory, Host process, service directory and a Claude provider, and whose port
 * is the service's published Router listener.
 */
export function requireClaudeRouteBinding(binding: ClaudeRouteBinding): {
  readonly auditFile: string;
  readonly routerPort: number;
} {
  const directory = binding.hostRunDirectory;
  const production = join(homedir(), ".codex-router");
  if (!isAbsolute(directory) || directory === production || directory.startsWith(`${production}/`))
    throw new Error("the Claude route window needs an absolute non-production Host run directory");
  const status = lstatSync(directory);
  if (!status.isDirectory() || realpathSync(directory) !== directory || !ownerPrivate(status))
    throw new Error("Host run directory is not an owner-private canonical directory");
  const markerPath = join(directory, HOST_MARKER);
  const markerStatus = lstatSync(markerPath);
  if (!markerStatus.isFile() || !ownerPrivate(markerStatus) || markerStatus.size > MAX_MARKER_BYTES)
    throw new Error("Host context marker is not a small owner-private regular file");
  const marker = hostMarkerSchema.parse(JSON.parse(readFileSync(markerPath, "utf8")));
  if (
    marker.runDirectory !== directory ||
    marker.hostPid !== binding.hostPid ||
    marker.serviceDirectory !== binding.serviceDirectory ||
    marker.claudeProvider === null
  )
    throw new Error("Host context marker does not name the declared Claude Host and service");
  const listener = serviceListenerSchema.parse(
    JSON.parse(readFileSync(join(binding.serviceDirectory, "service.json"), "utf8")),
  );
  const port = /:(\d+)$/u.exec(listener.routerProxyEndpoint)?.[1];
  if (port === undefined || Number(port) !== marker.port)
    throw new Error("Host Router port is not the service's published listener");
  return { auditFile: join(directory, AUDIT_FILE), routerPort: marker.port };
}

/** Opens the audit file without following links and checks the opened file is the listed one. */
function openAuditFile(path: string): {
  readonly fd: number;
  readonly size: number;
  readonly device: string;
  readonly inode: string;
} {
  const listed = lstatSync(path, { bigint: true });
  if (!listed.isFile() || !ownerPrivate({ mode: Number(listed.mode), uid: Number(listed.uid) }))
    throw new Error("Router audit file is not an owner-private regular file");
  const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  const status = fstatSync(fd, { bigint: true });
  if (status.dev !== listed.dev || status.ino !== listed.ino) {
    closeSync(fd);
    throw new Error("Router audit file changed while it was opened");
  }
  return {
    fd,
    size: Number(status.size),
    device: status.dev.toString(),
    inode: status.ino.toString(),
  };
}

/** Reads `[start, end)` from the descriptor in bounded chunks. */
function readRange(fd: number, start: number, end: number): Buffer {
  const chunks: Buffer[] = [];
  for (let offset = start; offset < end; offset += READ_CHUNK_BYTES) {
    const chunk = Buffer.alloc(Math.min(READ_CHUNK_BYTES, end - offset));
    const read = readSync(fd, chunk, 0, chunk.length, offset);
    if (read !== chunk.length) throw new Error("Router audit file shrank while it was read");
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

function isMissing(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

const sha256 = (bytes: Buffer): string => createHash("sha256").update(bytes).digest("hex");

/** The runner's record right before the submission Enter: identity, length and prefix digest. */
export function captureClaudeRouteBaseline(
  binding: ClaudeRouteBinding,
  target: {
    readonly afterScene: ClaudeRouteBaseline["after_scene"];
    readonly providerSessionId: string;
    readonly nativeThreadId: string;
  },
): ClaudeRouteBaseline {
  const { auditFile, routerPort } = requireClaudeRouteBinding(binding);
  const record = {
    kind: "claude-route-baseline" as const,
    after_scene: target.afterScene,
    host_run_directory: binding.hostRunDirectory,
    audit_file: auditFile,
    host_pid: binding.hostPid,
    service_directory: binding.serviceDirectory,
    router_port: routerPort,
    provider_session_id: target.providerSessionId,
    native_thread_id: target.nativeThreadId,
    max_prefix_bytes: binding.maxPrefixBytes,
    max_segment_bytes: binding.maxSegmentBytes,
  };
  let opened;
  try {
    opened = openAuditFile(auditFile);
  } catch (error) {
    if (isMissing(error))
      return claudeRouteBaselineSchema.parse({ ...record, file: { state: "absent" } });
    throw error;
  }
  try {
    const length = opened.size;
    if (length > binding.maxPrefixBytes)
      throw new Error("Router audit prefix exceeds its declared bound");
    return claudeRouteBaselineSchema.parse({
      ...record,
      file: {
        state: "present",
        device: opened.device,
        inode: opened.inode,
        byte_length: length,
        prefix_sha256: sha256(readRange(opened.fd, 0, length)),
      },
    });
  } finally {
    closeSync(opened.fd);
  }
}

/** Every appended line is one complete audit row; only categorical fields are kept. */
export function summarizeAuditSegment(segment: Buffer): Omit<ClaudeRouteWindow, "baseline_state"> {
  const text = segment.toString("utf8");
  if (text.length > 0 && !text.endsWith("\n"))
    throw new Error("Router audit segment ends inside a row");
  const lines = text.length === 0 ? [] : text.slice(0, -1).split("\n");
  const categories = new Map<string, AuditCategory>();
  for (const line of lines) {
    let row: AuditRow;
    try {
      row = auditRowSchema.parse(JSON.parse(line));
    } catch {
      throw new Error("Router audit segment holds a malformed row");
    }
    const category = {
      route_kind: row.route_kind,
      transport_kind: row.transport_kind,
      local_auth_result: row.local_auth_result,
      outcome: row.outcome,
      decision_reason: row.decision_reason,
      response_commit_state: row.response_commit_state,
      error_class: row.error_class ?? null,
      account_hash_present: row.account_hash !== undefined,
    };
    const key = JSON.stringify(category);
    categories.set(key, { ...category, count: (categories.get(key)?.count ?? 0) + 1 });
  }
  return {
    segment_bytes: segment.length,
    segment_sha256: sha256(segment),
    rows: lines.length,
    categories: [...categories.values()],
  };
}

/**
 * After the same-session reply readback: the file is still the recorded one with its prefix
 * intact, and only the bounded segment appended since the baseline is read and summarized.
 */
export function readClaudeRouteWindow(
  baseline: ClaudeRouteBaseline,
  binding: ClaudeRouteBinding,
): ClaudeRouteWindow {
  const { auditFile, routerPort } = requireClaudeRouteBinding(binding);
  if (
    auditFile !== baseline.audit_file ||
    routerPort !== baseline.router_port ||
    binding.maxPrefixBytes !== baseline.max_prefix_bytes ||
    binding.maxSegmentBytes !== baseline.max_segment_bytes
  )
    throw new Error("Claude route binding differs from the recorded baseline");
  let opened;
  try {
    opened = openAuditFile(auditFile);
  } catch (error) {
    if (isMissing(error)) throw new Error("Router audit file is missing after the reply");
    throw error;
  }
  try {
    const size = opened.size;
    let start = 0;
    if (baseline.file.state === "present") {
      if (opened.device !== baseline.file.device || opened.inode !== baseline.file.inode)
        throw new Error("Router audit file was replaced after the baseline");
      if (size < baseline.file.byte_length)
        throw new Error("Router audit file was truncated after the baseline");
      if (
        sha256(readRange(opened.fd, 0, baseline.file.byte_length)) !== baseline.file.prefix_sha256
      )
        throw new Error("Router audit prefix changed after the baseline");
      start = baseline.file.byte_length;
    }
    if (size - start > baseline.max_segment_bytes)
      throw new Error("Router audit segment exceeds its declared bound");
    return {
      baseline_state: baseline.file.state,
      ...summarizeAuditSegment(readRange(opened.fd, start, size)),
    };
  } finally {
    closeSync(opened.fd);
  }
}
