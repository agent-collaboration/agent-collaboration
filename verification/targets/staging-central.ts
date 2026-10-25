import { CentralOwner, environmentValue } from "../clients/central-owner.ts";
import type { Target } from "../declaration.ts";
import type { CentralTarget } from "./central-target.ts";

export class StagingCentral implements CentralTarget {
  readonly origin: string;
  readonly owner: CentralOwner;
  constructor(target: Extract<Target, { kind: "staging" }>) {
    this.origin = target.origin;
    this.owner = new CentralOwner(target.origin, {
      "cf-access-token": environmentValue(target.ownerTokenEnv),
    });
  }
  async close(): Promise<void> {
    /* Attach-only: no deployed process belongs to this runner. */
  }
}
