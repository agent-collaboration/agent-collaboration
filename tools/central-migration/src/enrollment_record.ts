// PD3: an old enrollment row in main's shape, or the reason main's decoder
// would refuse it (collaboration-central-core machine_enrollment_record.rs
// `StoredMachine::decode`, with ServiceId, MachineLabel and AccessClientId's
// own rules).

import type { NewRow } from "./fleet_conversion.ts";
import type { ReadRow } from "./storage_objects.ts";

const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
/** message_board ServiceId: a canonical lowercase RFC UUID of any version. */
const CANONICAL_UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const MAX_MACHINE_LABEL_SCALARS = 120;
// Rust's char::is_control: the Cc category.
const CONTROL = /\p{Cc}/u;

/** AccessClientId::new and valid_hash: nonempty, already trimmed, no control characters. */
const trimmedText = (value: string): boolean =>
  value !== "" && value.trim() === value && !CONTROL.test(value);

const RFC3339 =
  /^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?(?:([Zz])|([+-])(\d{2}):?(\d{2}))$/;

/**
 * The instant chrono's `DateTime<Utc>` parse accepts, or null: a full date and
 * time with an offset. Returns whether the instant is a whole millisecond.
 */
export function parseUtcInstant(value: string): { readonly wholeMillisecond: boolean } | null {
  const match = RFC3339.exec(value);
  if (match === null) return null;
  const [, year, month, day, hour, minute, second, fraction = "", , , offsetHour, offsetMinute] =
    match;
  const y = Number(year);
  const m = Number(month);
  const d = Number(day);
  const daysInMonth = new Date(Date.UTC(y, m, 0)).getUTCDate();
  if (m < 1 || m > 12 || d < 1 || d > daysInMonth) return null;
  if (Number(hour) > 23 || Number(minute) > 59 || Number(second) > 59) return null;
  if (offsetHour !== undefined && (Number(offsetHour) > 23 || Number(offsetMinute) > 59))
    return null;
  return { wholeMillisecond: /^\d{0,3}0*$/.test(fraction) };
}

export function enrollmentRow(row: ReadRow, runTimeText: string): NewRow | string {
  const field = (name: string): string | null =>
    typeof row[name] === "string" ? (row[name] as string) : null;
  const peer = field("peer_id");
  if (peer === null || !UUID_V7.test(peer)) return "peer_id is not a canonical UUIDv7";
  const service = field("service_id");
  if (service === null || !CANONICAL_UUID.test(service))
    return "service_id is not a canonical UUID";
  const label = field("machine_label");
  // MachineLabel::try_from trims and bounds; main refuses a label that doesn't round-trip.
  if (
    label === null ||
    label.trim() === "" ||
    label.trim() !== label ||
    [...label].length > MAX_MACHINE_LABEL_SCALARS
  )
    return "machine_label is empty, padded or longer than 120 characters";
  const accessClient = field("access_client_id");
  if (accessClient === null || !trimmedText(accessClient))
    return "access_client_id is empty, padded or has control characters";
  const hash = field("credential_hash");
  if (hash === null || !trimmedText(hash))
    return "credential_hash is empty, padded or has control characters";
  const state = field("state");
  if (state !== "enrolled" && state !== "revoked")
    return `state ${String(row.state)} is neither enrolled nor revoked`;
  const expires = field("expires_at");
  if (expires === null || parseUtcInstant(expires) === null)
    return "expires_at is not an RFC 3339 date and time with an offset";
  const lastSeen = row.last_seen;
  if (lastSeen !== null) {
    const instant = typeof lastSeen === "string" ? parseUtcInstant(lastSeen) : null;
    if (instant === null || !instant.wholeMillisecond)
      return "last_seen is not an RFC 3339 time at a whole millisecond";
  }
  for (const name of ["last_revocation_operation", "last_enrollment_operation"]) {
    const value = row[name];
    if (value !== null && !(typeof value === "string" && UUID_V7.test(value)))
      return `${name} is not a canonical UUIDv7`;
  }
  return {
    machine_id: peer,
    service_id: service,
    machine_label: label,
    access_client_id: accessClient,
    credential_hash: hash,
    state,
    expires_at: expires,
    last_seen_at: (lastSeen as string | null) ?? null,
    last_revocation_operation_id: (row.last_revocation_operation as string | null) ?? null,
    last_enrollment_operation_id: (row.last_enrollment_operation as string | null) ?? null,
    created_at: runTimeText,
    updated_at: runTimeText,
  };
}
