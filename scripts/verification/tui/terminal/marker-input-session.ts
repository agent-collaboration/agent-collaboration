import { InputConsumptionReadback } from '../sessions/input-consumption-readback.ts';
/** Every injected input is acknowledged by the real owner and paired to its own flushed frame. */
import type {
	InputConsumptionExpectation,
	InputDispatchReceipt,
} from '../sessions/input-consumption-schema.ts';
import type { GhosttyInputProfile } from './example-ghostty-keybinds.ts';
import type { CellMouseEvent } from './ghostty-input-encoding.ts';
import { GhosttyJourneyInput, requireDelivered } from './ghostty-journey-input.ts';
import type { DispatchFrameObservation } from './input-dispatch-frames.ts';
import {
	requireHandoffChildEntry,
	requireHandoffChildExit,
	requireHandoffRemount,
	observeHandoffFact,
} from './loop-ending-handoff.ts';
import type { LoopEndingHandoff } from './loop-ending-handoff.ts';
import type { PtyGridDriver } from './pty-grid-driver.ts';

export interface MarkedInputEvidence {
	readonly receipts: readonly InputDispatchReceipt[];
	readonly frames: readonly DispatchFrameObservation[];
	/** A burst never observes the state between its inputs. Send separately to assert it. */
	readonly observation: { readonly kind: 'after-input' | 'after-burst'; readonly sequence: number };
}
export interface LoopEndingHandoffEvidence {
	readonly driverPid: number;
	readonly receipts: readonly InputDispatchReceipt[];
	readonly childEntry: ReturnType<typeof requireHandoffChildEntry>;
	readonly childExit: ReturnType<typeof requireHandoffChildExit>;
	readonly remount: ReturnType<typeof requireHandoffRemount>;
	readonly firstFrame: MarkedInputEvidence;
}
export class MarkerInputSession {
	private readonly driver: PtyGridDriver;
	private readonly reader: InputConsumptionReadback;
	readonly input: GhosttyJourneyInput;
	constructor(options: {
		readonly driver: PtyGridDriver;
		readonly receipt: string;
		readonly profile: GhosttyInputProfile;
	}) {
		this.driver = options.driver;
		this.reader = InputConsumptionReadback.open(options.receipt);
		this.input = new GhosttyJourneyInput(options.driver, options.profile);
	}
	async observe(options: {
		readonly expected: readonly InputConsumptionExpectation[];
		readonly inject: () => Promise<void>;
	}): Promise<MarkedInputEvidence> {
		const finalSequence = this.reader.sequence + options.expected.length;
		const receipts = await this.reader.observeInput({
			expected: options.expected,
			timeoutMs: 8000,
			inject: async () => {
				await options.inject();
				// The producer flushes its receipt before publishing this exact marker.
				// Re-read after that causal fact even if file notifications coalesce.
				await this.driver.waitForDispatchFrame(finalSequence);
			},
		});
		const frames = await Promise.all(
			receipts.map((receipt) => this.driver.waitForDispatchFrame(receipt.sequence)),
		);
		return {
			receipts,
			frames,
			observation: {
				kind: receipts.length === 1 ? 'after-input' : 'after-burst',
				sequence: finalSequence,
			},
		};
	}
	press(
		key: string,
		expected: readonly InputConsumptionExpectation[],
	): Promise<MarkedInputEvidence> {
		return this.observe({
			expected,
			inject: async () => {
				const delivered =
					Array.from(key).length === 1 ? await this.input.text(key) : await this.input.key(key);
				requireDelivered(delivered);
			},
		});
	}
	text(
		text: string,
		expected: readonly InputConsumptionExpectation[],
	): Promise<MarkedInputEvidence> {
		return this.observe({
			expected,
			inject: async () => {
				requireDelivered(await this.input.text(text));
			},
		});
	}
	pointer(
		event: CellMouseEvent,
		expected: readonly InputConsumptionExpectation[],
	): Promise<MarkedInputEvidence> {
		return this.observe({
			expected,
			inject: async () => {
				requireDelivered(await this.input.mouse(event));
			},
		});
	}
	resize(
		columns: number,
		rows: number,
		expected: readonly InputConsumptionExpectation[],
	): Promise<MarkedInputEvidence> {
		return this.observe({ expected, inject: () => this.driver.resize(columns, rows) });
	}
	burst(
		bytes: Uint8Array,
		expected: readonly InputConsumptionExpectation[],
	): Promise<MarkedInputEvidence> {
		return this.observe({ expected, inject: () => this.driver.send(bytes) });
	}
	/** Loop-ending input has no promised marker: exact producer ACK, EOF and real exit settle it. */
	async exit(options: {
		readonly expected: readonly InputConsumptionExpectation[];
		readonly inject: () => Promise<void>;
	}): Promise<{
		readonly receipts: readonly InputDispatchReceipt[];
		readonly exitCode: number | null;
	}> {
		let exitCode: number | null = null;
		const receipts = await this.reader.observeInput({
			expected: options.expected,
			timeoutMs: 8000,
			inject: async () => {
				await options.inject();
				exitCode = await this.driver.drainUntilExit();
			},
		});
		if (exitCode !== 0)
			throw new Error('flow product or PTY bridge did not exit successfully: ' + String(exitCode));
		this.reader.assertComplete();
		return { receipts, exitCode };
	}
	assertComplete(): readonly InputDispatchReceipt[] {
		return this.reader.assertComplete();
	}
	/** Only the UI loop ends. Keep this PTY owner through the child and its fresh quota mount. */
	async handoff(
		options: LoopEndingHandoff & {
			readonly expected: readonly InputConsumptionExpectation[];
			readonly inject: () => Promise<void>;
		},
	): Promise<LoopEndingHandoffEvidence> {
		if (options.expected.length !== 1 || options.firstFrame.expected.length !== 1)
			throw new Error('handoff requires one action ACK and one first-frame barrier');
		const observation = new AbortController();
		const deadline = setTimeout(
			() => observation.abort(new Error('handoff lifecycle fact unavailable')),
			8000,
		);
		try {
			const driverPid = this.driver.processId();
			const beforeByteOffset = this.driver.bytes().length;
			const beforeGeneration = this.driver.frameGeneration();
			let entered: unknown;
			const receipts = await this.reader.observeInput({
				expected: options.expected,
				timeoutMs: 8000,
				inject: async () => {
					await options.inject();
					// A real entry fact forces a re-read even if receipt notifications coalesce.
					entered = await observeHandoffFact(options.childEntered, observation.signal);
				},
			});
			const childEntry = requireHandoffChildEntry(entered, options.expectedChildArguments);
			const childExit = requireHandoffChildExit(
				await observeHandoffFact(options.childExited, observation.signal),
				options.expectedChildExitCode,
			);
			const remount = requireHandoffRemount(
				await observeHandoffFact(options.remounted, observation.signal),
				options.previousMount,
			);
			const actionReceipt = receipts[0];
			if (actionReceipt === undefined) throw new Error('handoff exact action receipt absent');
			await this.driver.waitForGrid('handoff fresh quota scene', options.ready, beforeGeneration);
			this.driver.acknowledgeLoopEndingHandoff(actionReceipt.sequence);
			const firstFrame = await this.press(options.firstFrame.key, options.firstFrame.expected);
			const latest = firstFrame.frames.at(-1);
			if (
				this.driver.processId() !== driverPid ||
				latest === undefined ||
				!options.ready(latest.grid) ||
				latest.binding.frameEndByteOffset <= beforeByteOffset
			)
				throw new Error('handoff first frame is not the fresh quota state on the same PTY owner');
			return { driverPid, receipts, childEntry, childExit, remount, firstFrame };
		} finally {
			clearTimeout(deadline);
			observation.abort(new Error('handoff observation settled'));
		}
	}
}
