// The controlled peer binds this private release socket before publishing HOLDING.
// The controller names the held Session owner; no file polling or marker cleanup.
import { signalHeldRelease } from "../../../fixtures/held-release-control.ts";

/** Release only the named Session's already-admitted held work. */
export async function releaseHeldWork(socketPath: string, ownerId: string): Promise<void> {
  const reply = await signalHeldRelease({ socketPath, ownerId });
  if (reply.kind !== "released") throw new Error("controlled peer refused the release owner");
}
