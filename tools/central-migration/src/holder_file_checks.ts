// S2's comparison of each holder Mac's retained file with central's records,
// and the Copy facts the inventory reports for it.

import type { CopyRecord } from "./lease_records.ts";
import { positionBefore, type ExactPosition, type MacFileEvidence } from "./mac_file_evidence.ts";
import { parseExact } from "./exact_json.ts";

/** A Copy's facts as central stores them, for S2's comparison with the Mac's file. */
export interface CopyReport {
  readonly state: string;
  readonly role: string | null;
  readonly fence: bigint | null;
  readonly holder: string | null;
  readonly start: ExactPosition | null;
  readonly acked: ExactPosition | null;
  readonly schema: { readonly version: bigint; readonly fingerprint: string } | null;
}

/** The authoritative receiver old central is configured with (O RegistryConfig.authoritativeReceiver). */
export function configuredReceiverOf(registryConfig: string | undefined): string | null {
  let receiver: unknown;
  try {
    receiver = (parseExact(registryConfig ?? "") as { authoritativeReceiver?: unknown } | null)
      ?.authoritativeReceiver;
  } catch {
    return null;
  }
  return typeof receiver === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(receiver)
    ? receiver
    : null;
}

export function copyReportOf(record: CopyRecord): CopyReport {
  if (record.state === "retired") {
    return {
      state: "retired",
      role: null,
      fence: null,
      holder: null,
      start: null,
      acked: null,
      schema: null,
    };
  }
  const { status } = record;
  return {
    state: record.state,
    role: status.role,
    fence: status.fence,
    holder: status.holder,
    start: status.start,
    acked: status.acked,
    schema: status.schema,
  };
}

/** S2 against central's records: same writer and lease, same receiver, acknowledged position retained. */
export function compareMacFile(
  project: string,
  holder: string,
  leaseNumber: bigint | null,
  copy: CopyReport | null,
  configuredReceiver: string | null,
  file: MacFileEvidence | undefined,
): string | null {
  if (file === undefined) return "no retained-file facts from the holder Mac were supplied";
  if (file.holder !== holder || file.lease !== leaseNumber)
    return `the Mac's file is writer ${file.holder} at lease ${file.lease}; central's registry says ${holder} at ${String(leaseNumber)}`;
  if (configuredReceiver !== null && file.senderReceiver !== configuredReceiver)
    return `the Mac's sender is registered with receiver ${file.senderReceiver}; central is configured with ${configuredReceiver}`;
  const acked = copy?.acked ?? null;
  if (acked === null) return `copy ${project} has no acknowledged position`;
  if (positionBefore(acked, file.retained.floor) || positionBefore(file.retained.head, acked))
    return `copy's acknowledged position ${acked.epoch}/${acked.sequence} is outside the Mac's retained window ${file.retained.floor.epoch}/${file.retained.floor.sequence} through ${file.retained.head.epoch}/${file.retained.head.sequence}`;
  return null;
}
