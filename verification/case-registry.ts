import type { CaseRegistration } from "./coverage.ts";
import type { PreparedRun } from "./run-setup.ts";
import { ObservationOpen, type Receipt } from "./receipt.ts";
import { messagesAndNotifications, messageNotificationCase } from "./cases/message-notification.ts";
import { centralOutage, centralOutageCase } from "./cases/central-outage.ts";
import { heldCopy, heldCopyCase } from "./cases/held-copy.ts";
import { remoteWakes, remoteWakesCase } from "./cases/remote-wakes.ts";
import { remoteWakeLimits, remoteWakeLimitsCase } from "./cases/remote-wake-limits.ts";
import { remoteWakeExpiry, remoteWakeExpiryCase } from "./cases/remote-wake-expiry.ts";
import { messageCancellation, messageCancellationCase } from "./cases/message-cancellation.ts";
import { forwardedPosts, forwardedPostsCase } from "./cases/forwarded-posts.ts";

export interface RunnableCase extends CaseRegistration {
  run(context: PreparedRun, receipt: Receipt): Promise<void>;
}
export function casesFor(): readonly RunnableCase[] {
  const cases: RunnableCase[] = [
    // Before either direction's send can warm the installation directory.
    {
      ...centralOutageCase,
      run: (context, receipt): Promise<void> => {
        const source = context.machines[1],
          target = context.machines[0];
        const edge = context.gates.get(source.installation.machine.name);
        if (edge === undefined) throw new ObservationOpen("source session gate is not bound");
        return centralOutage({ source, target, edge, receipt });
      },
    },
    {
      ...messageNotificationCase,
      run: (context, receipt): Promise<void> =>
        messagesAndNotifications({ machines: context.machines, gates: context.gates, receipt }),
    },
  ];
  cases.push({
    ...heldCopyCase,
    run: heldCopy,
  });
  cases.push(
    { ...remoteWakesCase, run: remoteWakes },
    { ...remoteWakeLimitsCase, run: remoteWakeLimits },
    { ...remoteWakeExpiryCase, run: remoteWakeExpiry },
    { ...messageCancellationCase, run: messageCancellation },
    // F observes delivery; its native and Worker non-retention proof is separate.
    { ...forwardedPostsCase, run: forwardedPosts },
  );
  return cases;
}
