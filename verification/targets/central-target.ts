import type { CentralOwner } from "../clients/central-owner.ts";
export interface CentralTarget {
  readonly origin: string;
  readonly owner: CentralOwner;
  close(): Promise<void>;
}
