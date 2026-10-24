import type { CaseRegistration } from "./coverage.ts";
import type { PreparedRun } from "./run-setup.ts";
import { ObservationOpen, type Receipt } from "./receipt.ts";
import { messagesAndNotifications, messageNotificationCase } from "./cases/message-notification.ts";
import { centralOutage, centralOutageCase } from "./cases/central-outage.ts";
import { heldCopy, heldCopyCase } from "./cases/held-copy.ts";

export interface RunnableCase extends CaseRegistration {
  run(context: PreparedRun, receipt: Receipt): Promise<void>;
}
export function casesFor(): readonly RunnableCase[] {
  const cases: RunnableCase[] = [
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
  cases.push({
    ...centralOutageCase,
    run: (context, receipt): Promise<void> => {
      const source = context.machines[1],
        target = context.machines[0];
      const edge = context.gates.get(source.installation.machine.name);
      if (edge === undefined) throw new ObservationOpen("source session gate is not bound");
      return centralOutage({ source, target, edge, receipt });
    },
  });
  return cases;
}
