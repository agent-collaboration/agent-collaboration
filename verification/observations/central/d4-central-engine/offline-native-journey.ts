/** Source-bound observation callbacks for the real ten-message Router custody drive. */
import { createInterface } from "node:readline";
import { readFile } from "node:fs/promises";
import { EventEmitter } from "node:events";
import { z } from "zod";
import type { D1HarnessJourney, D1Machine } from "../d1-harness-plan.ts";
import type { Receipt } from "../../../receipt.ts";
import type { OwnedProcess } from "../../../machines/owned-process.ts";
import { CentralOwner } from "../../../clients/central-owner.ts";
import { decode } from "../../../exact-json.ts";
import { withinFixtureDeadline } from "../../../fixtures/fixture-control-socket.ts";
import { nativeTurnStarts } from "../d1-native-activity.ts";
import { persistEmptyRecipient, readUnloadedPersistedRecipient } from "../passive-recipient.ts";
import { readD1Stores } from "../d1-sqlite-readback.ts";
import { D4EventSequence } from "./event-observation.ts";
import { D4WireCapture } from "./wire-capture.ts";
import { bindLiveSession, LiveSessionBindingPending } from "./live-session-binding.ts";
import { joinWireEvents, type WireConnectionBinding } from "./wire-event-join.ts";
import { replayWindowBytes } from "./window-readback.ts";
import { driveOfflineTen, type OfflineHeldReadbacks } from "./offline-custody-drive.ts";
import { custodyManifestFromSender, observeHeldCustody } from "./custody-observation.ts";
import { observePerSenderPushOrder } from "./push-order-observation.ts";
import type { D4OfflineHarnessPlan } from "./offline-harness-plan.ts";

type MachineName = "a" | "b" | "c";
interface MachineCapture {
  readonly wire: D4WireCapture;
  revision: number;
}

export class OfflineNativeJourney implements D1HarnessJourney {
  readonly declaration: D4OfflineHarnessPlan;
  private readonly sequence = new D4EventSequence();
  private readonly wireChanges = new EventEmitter();
  private readonly captures = new Map<MachineName, MachineCapture>();
  private readonly bindings = new Map<string, WireConnectionBinding>();
  private failure: unknown;
  private stderrClosed: Promise<void> | undefined;
  private held: OfflineHeldReadbacks | undefined;

  constructor(plan: D4OfflineHarnessPlan) {
    this.declaration = plan;
    for (const name of ["a", "b", "c"] as const)
      this.captures.set(name, { wire: new D4WireCapture(), revision: 0 });
  }
  private capture(name: MachineName): MachineCapture {
    const capture = this.captures.get(name);
    if (capture === undefined) throw new Error("machine capture absent");
    return capture;
  }
  private healthy(): void {
    if (this.failure !== undefined)
      throw new Error("native S1 capture failed", { cause: this.failure });
  }
  centralStarted(central: OwnedProcess): void {
    const stderr = central.child.stderr;
    if (stderr === null) throw new Error("central stderr capture unavailable");
    const lines = createInterface({ input: stderr });
    this.stderrClosed = new Promise<void>((resolve): void => {
      lines.once("close", resolve);
    });
    lines.on("line", (line): void => {
      try {
        this.sequence.observeLine(line);
      } catch (error: unknown) {
        this.failure ??= error;
      }
    });
  }
  sessionObservation(name: MachineName): ReturnType<D1HarnessJourney["sessionObservation"]> {
    const capture = this.capture(name);
    return {
      protocol: "batches",
      observeBytes: (record): void => {
        capture.wire.observe(record);
        capture.revision += 1;
        this.wireChanges.emit(name);
      },
    };
  }
  private async waitForWire(
    name: MachineName,
    revision: number,
    milliseconds: number,
  ): Promise<void> {
    const capture = this.capture(name);
    await new Promise<void>((resolve, reject): void => {
      const cleanup = (): void => {
        clearTimeout(timer);
        this.wireChanges.off(name, changed);
      };
      const changed = (): void => {
        if (capture.revision !== revision) {
          cleanup();
          resolve();
        }
      };
      const timer = setTimeout((): void => {
        cleanup();
        reject(new Error("live peer binding deadline"));
      }, milliseconds);
      this.wireChanges.on(name, changed);
      changed();
    });
  }
  async connectionReady(input: Parameters<D1HarnessJourney["connectionReady"]>[0]): Promise<void> {
    const fixture = z
      .object({ origin: z.literal(input.origin), assertion: z.string().min(1) })
      .parse(decode(await readFile(`${input.centralRoot}/owner-fixture.json`, "utf8")));
    const owner = new CentralOwner(input.origin, { "Cf-Access-Jwt-Assertion": fixture.assertion });
    const readAdmission = async (): Promise<unknown> => {
      const reply = await owner.response({
        path: "/__conformance/admission",
        method: "POST",
        body: { kind: "sessionConnections", value: { service: input.serviceId } },
      });
      if (reply.status !== 200) throw new Error("actual owner admission read failed");
      return reply.body;
    };
    const capture = this.capture(input.name),
      deadline = Date.now() + 20_000;
    while (Date.now() < deadline) {
      this.healthy();
      input.gate.requireObservation();
      capture.wire.requireHealthy();
      const admissionBefore = await readAdmission(),
        revision = capture.revision;
      const trace = capture.wire.transcript(),
        admissionAfter = await readAdmission();
      try {
        const binding = bindLiveSession({
          trace,
          peer: input.peer,
          admissionBefore,
          admissionAfter,
        });
        const previous = this.bindings.get(binding.physical);
        if (
          previous !== undefined &&
          (previous.connection !== binding.connection || previous.session !== binding.session)
        )
          throw new Error("actual physical/platform binding changed");
        this.bindings.set(binding.physical, binding);
        await input.receipt.record("d4ActualPeerBinding", {
          machine: input.name,
          peer: input.peer,
          binding,
          admissionBefore,
          admissionAfter,
        });
        return;
      } catch (error: unknown) {
        if (!(error instanceof LiveSessionBindingPending)) throw error;
        await this.waitForWire(input.name, revision, Math.max(1, deadline - Date.now()));
      }
    }
    throw new Error("actual live peer binding unavailable");
  }
  async recipientStarted(
    input: Parameters<D1HarnessJourney["recipientStarted"]>[0],
  ): Promise<void> {
    if (nativeTurnStarts([input.native.events]) !== 0)
      throw new Error("native input preceded recipient preparation");
    const persisted = await persistEmptyRecipient({
      native: input.native,
      threadId: input.recipient.sessionId,
    });
    const stored = await readUnloadedPersistedRecipient({
      native: input.native,
      threadId: input.recipient.sessionId,
      home: input.home,
    });
    if (nativeTurnStarts([input.native.events]) !== 0)
      throw new Error("native input started during empty persistence");
    await input.receipt.record("d4EmptyPersistedRecipientWithoutInput", {
      recipient: input.recipient,
      ...persisted,
      stored,
    });
  }
  async drive(input: Parameters<D1HarnessJourney["drive"]>[0]): Promise<void> {
    this.healthy();
    const sender = input.machines[0],
      recipient = input.machines[1];
    if (sender === undefined || recipient === undefined)
      throw new Error("real custody actors absent");
    this.held = await driveOfflineTen({ sender, recipient, receipt: input.receipt });
  }
  private async saveWire(machines: readonly D1Machine[], receipt: Receipt): Promise<void> {
    await receipt.save("d4-events.json", this.sequence.records());
    await receipt.save("d4-constructions.json", this.sequence.constructionRecords());
    await receipt.save("d4-connection-bindings.json", [...this.bindings.values()]);
    for (const [index, name] of (["a", "b", "c"] as const).entries()) {
      const machine = machines[index],
        trace = this.capture(name).wire.transcript();
      await receipt.save(`d4-wire-${name}.json`, { trace });
      if (machine === undefined) throw new Error("capture lacks its real machine");
      machine.gate.requireObservation();
      machine.gate.audit.requireComplete();
      machine.gate.outgoingAudit.requireComplete();
      const physical = new Set(
        trace
          .split("\n")
          .filter(Boolean)
          .map(
            (line): string => z.object({ connection: z.string() }).parse(decode(line)).connection,
          ),
      );
      const joined = joinWireEvents({
        trace,
        events: this.sequence.records(),
        connections: [...this.bindings.values()].filter((binding): boolean =>
          physical.has(binding.physical),
        ),
      });
      await receipt.save(`d4-joined-${name}.json`, {
        ...joined,
        window: joined.matchedSends === 0 ? null : replayWindowBytes(joined.trace),
      });
    }
  }
  async ownersStopped(input: Parameters<D1HarnessJourney["ownersStopped"]>[0]): Promise<void> {
    if (this.stderrClosed !== undefined) await withinFixtureDeadline(this.stderrClosed, 20_000);
    await this.saveWire(input.machines, input.receipt);
    this.healthy();
    this.sequence.requireComplete();
    const held = this.held,
      sender = input.machines[0],
      recipient = input.machines[1];
    if (held === undefined || sender === undefined || recipient === undefined)
      throw new Error("custody drive did not finish");
    const output = `${input.receipt.directory}/d4-owner-readback.json`;
    await readD1Stores({
      repository: this.declaration.repository,
      runtimeDirectory: this.declaration.runtimeDirectory,
      databases: [
        { role: "sender", path: `${sender.serviceDirectory}/interaction.sqlite` },
        { role: "recipient", path: `${recipient.serviceDirectory}/interaction.sqlite` },
      ],
      itemIds: [...held.items],
      output,
    });
    const raw = decode(await readFile(output, "utf8")),
      expected = custodyManifestFromSender({ itemIds: held.items, readback: raw });
    const custody = observeHeldCustody({
      journey: "J6",
      expected,
      inboxReadback: raw,
      recipientViews: held.recipientViews,
      senderViews: held.senderViews,
    });
    const order = observePerSenderPushOrder({
      submitted: held.items.map((item) => ({ item, sender: sender.recipient.endpoint.serviceId })),
      observedItems: recipient.gate.audit.snapshot().pushedItemIds.items,
    });
    await input.receipt.save("d4-held-custody.json", { custody, order, runtimeQualified: false });
  }
}
