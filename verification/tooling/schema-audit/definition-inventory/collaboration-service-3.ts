import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/collaboration-service/src/codex_session_inventory.rs",
    line: 44,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "InventoryCursor {endpoint, view, generation, expiresAt, nativeCursor, storedTime, storedId, includeEmptySessions, scope, source, query}: private struct serialized to base64url JSON by encode (l315-321) and decoded from the request cursor (l225-233); the opaque continuation of native session_list pages; boundary encoding, no defect",
    usedBy: [
      {
        part: "N11",
      },
    ],
    duplicateGroup: "native-inventory-cursor",
    disposition: "boundary-encoding",
  },
];
