/** Native binding for authored flows: one Rust PTY owner, exact ACKs and marked original bytes. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import { requireDelivered } from '../../terminal/ghostty-journey-input.ts';
import type { DispatchFrameObservation } from '../../terminal/input-dispatch-frames.ts';
import { MarkerInputSession } from '../../terminal/marker-input-session.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { PtyGridDriver } from '../../terminal/pty-grid-driver.ts';
import type { PtyGridTarget } from '../../terminal/pty-grid-driver.ts';
import { gridLines } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { renderOriginal } from '../../terminal/terminal-original.ts';
import type { FlowCheck, FlowStep } from './flow-definition.ts';
import { executeFlow } from './flow-executor.ts';
import type {
	FlowCapabilities,
	FlowCheckpointEvidence,
	FlowExecutionResult,
	FlowInvocation,
	FlowRuntime,
	FlowStepEvidence,
} from './flow-executor.ts';
import { locatePaintedTarget } from './painted-flow-target.ts';
import type { PaintedFlowTarget, PaintedTargetLocation } from './painted-flow-target.ts';

export interface NativeFlowContext {
	readonly driver: PtyGridDriver;
	readonly input: MarkerInputSession;
	readonly frame: DispatchFrameObservation;
}
type ExpectedInput = readonly InputConsumptionExpectation[];
export interface NativeFlowKey {
	readonly key: string;
	readonly expected: ExpectedInput;
}
/** The owner and action are independent fixture declarations; coordinates come from painted cells. */
export type NativeFlowInputBinding =
	| { readonly kind: 'continuing' | 'exit'; readonly expected: ExpectedInput }
	| {
			readonly kind: 'click';
			readonly expected: (location: PaintedTargetLocation) => ExpectedInput;
			/** The declared action ends the loop on MouseDown; no release/frame can be required afterward. */
			readonly endsOnPress?: boolean;
	  }
	| {
			readonly kind: 'focus';
			readonly route: readonly NativeFlowKey[];
			readonly isFocused: (grid: TerminalGrid) => boolean;
			readonly stopWhenFocused?: boolean;
	  };
export interface NativeFlowBindings {
	readonly target: PtyGridTarget;
	readonly receipt: string;
	readonly original: {
		readonly chrome: string;
		readonly sourceHead: string;
		readonly binarySha256: string;
	};
	readonly ready: (grid: TerminalGrid) => boolean;
	readonly expectedKeyboardFlags?: number;
	/** A safe, declared key makes the initial scene marker-bound too. */
	readonly initialBarrier: NativeFlowKey;
	/** Async publication is checked before a later harmless, independently acknowledged frame. */
	readonly checkpointBarriers?: ReadonlyMap<
		string,
		{ readonly ready: (grid: TerminalGrid) => boolean; readonly publish: NativeFlowKey }
	>;
	readonly resizeExpected: (columns: number, rows: number) => ExpectedInput;
	readonly inputs: ReadonlyMap<number, NativeFlowInputBinding>;
	readonly targets: ReadonlyMap<string, (grid: TerminalGrid) => PaintedFlowTarget>;
	readonly fixtures: ReadonlyMap<
		string,
		(context: NativeFlowContext) => Promise<MarkedInputEvidence | null>
	>;
	readonly readbacks: ReadonlyMap<
		Extract<FlowStep, { kind: 'readback' }>['port'],
		(context: NativeFlowContext) => Promise<unknown>
	>;
	readonly checks: ReadonlyMap<
		FlowCheck,
		(context: NativeFlowContext, readback: unknown) => Promise<readonly string[]>
	>;
	readonly quit: NativeFlowKey;
}
function requireBinding<Binding>(binding: Binding | undefined, description: string): Binding {
	if (binding === undefined) throw new Error('native flow binding absent: ' + description);
	return binding;
}
/** Bindings carry transport/fixture facts, never a second copy of the journey's steps. */
export class NativeFlowRuntime implements FlowRuntime {
	private driver: PtyGridDriver | null = null;
	private input: MarkerInputSession | null = null;
	private frame: DispatchFrameObservation | null = null;
	private exited: FlowStepEvidence | null = null;
	private readonly bindings: NativeFlowBindings;
	private readonly invocation: FlowInvocation;
	constructor(invocation: FlowInvocation, bindings: NativeFlowBindings) {
		this.invocation = invocation;
		this.bindings = bindings;
		if (bindings.target.keys !== invocation.keys)
			throw new Error('native target key protocol differs from invocation');
		for (const [index, step] of invocation.journey.steps.entries()) {
			if (['press', 'type', 'click', 'focus'].includes(step.kind)) {
				const input = requireBinding(bindings.inputs.get(index), 'input step ' + index);
				if (
					step.kind === 'press' &&
					(input.kind === 'continuing' || input.kind === 'exit') &&
					input.expected.length !== 1
				)
					throw new Error(
						'a press step requires exactly one input ACK; author separate steps for intermediate state',
					);
				if (input.kind === 'focus' && input.route.some((key) => key.expected.length !== 1))
					throw new Error('each focus hop requires exactly one input ACK');
				if (
					step.kind === 'click' || step.kind === 'focus'
						? input.kind !== step.kind
						: !['continuing', 'exit'].includes(input.kind)
				)
					throw new Error('native input binding kind differs at step ' + index);
			}
		}
	}
	private context(): NativeFlowContext {
		if (this.driver === null || this.input === null || this.frame === null || this.exited !== null)
			throw new Error('native flow has no live marker-bound frame');
		return { driver: this.driver, input: this.input, frame: this.frame };
	}
	private retain(marked: MarkedInputEvidence): FlowStepEvidence {
		const latest = marked.frames.at(-1);
		if (latest === undefined) throw new Error('continuing input supplied no marked frame');
		if (latest.binding.sequence !== marked.observation.sequence)
			throw new Error('flow step did not retain its final input frame');
		this.frame = latest;
		return {
			files: marked.frames.map((frame) =>
				join(this.bindings.target.evidence, `dispatch-${frame.binding.sequence}-frame.json`),
			),
			dispatchSequences: marked.receipts.map((receipt) => receipt.sequence),
			inputObservation: marked.observation,
		};
	}
	async start(): Promise<void> {
		this.driver = await PtyGridDriver.start(this.bindings.target);
		await this.driver.waitForGrid('native authored-flow initial scene', this.bindings.ready);
		if (
			this.bindings.expectedKeyboardFlags !== undefined &&
			this.driver.inputModes().kittyFlags !== this.bindings.expectedKeyboardFlags
		)
			throw new Error('source-derived keyboard enhancement flags differ from native output');
		this.input = new MarkerInputSession({
			driver: this.driver,
			receipt: this.bindings.receipt,
			profile: this.invocation.journey.inputProfile ?? 'terminal-only',
		});
		const dimensions = this.invocation.grid.split('x').map(Number);
		const columns = dimensions[0],
			rows = dimensions[1];
		if (columns === undefined || rows === undefined) throw new Error('flow viewport absent');
		if (columns !== 120 || rows !== 36)
			this.retain(
				await this.input.resize(columns, rows, this.bindings.resizeExpected(columns, rows)),
			);
		this.retain(
			await this.input.press(
				this.bindings.initialBarrier.key,
				this.bindings.initialBarrier.expected,
			),
		);
	}
	private async inject(
		index: number,
		send: (input: MarkerInputSession) => Promise<void>,
	): Promise<FlowStepEvidence> {
		const { input } = this.context();
		const binding = requireBinding(this.bindings.inputs.get(index), 'input step ' + index);
		if (binding.kind !== 'continuing' && binding.kind !== 'exit')
			throw new Error('expected a key/text binding');
		if (binding.kind === 'exit') {
			const result = await input.exit({ expected: binding.expected, inject: () => send(input) });
			this.exited = {
				files: [this.bindings.receipt, join(this.bindings.target.evidence, 'terminal.ansi')],
				dispatchSequences: result.receipts.map((receipt) => receipt.sequence),
			};
			return this.exited;
		}
		return this.retain(
			await input.observe({ expected: binding.expected, inject: () => send(input) }),
		);
	}
	press(step: Extract<FlowStep, { kind: 'press' }>, index: number): Promise<FlowStepEvidence> {
		return this.inject(index, async (input) =>
			requireDelivered(
				Array.from(step.key).length === 1
					? await input.input.text(step.key)
					: await input.input.key(step.key),
			),
		);
	}
	type(step: Extract<FlowStep, { kind: 'type' }>, index: number): Promise<FlowStepEvidence> {
		return this.inject(index, async (input) => requireDelivered(await input.input.text(step.text)));
	}
	async click(
		step: Extract<FlowStep, { kind: 'click' }>,
		index: number,
	): Promise<FlowStepEvidence> {
		const { input, frame } = this.context();
		const binding = requireBinding(this.bindings.inputs.get(index), 'click step ' + index);
		if (binding.kind !== 'click') throw new Error('expected a click binding');
		const target = requireBinding(this.bindings.targets.get(step.target), step.target)(frame.grid);
		const location = locatePaintedTarget(frame.grid, target);
		if (binding.endsOnPress === true) {
			const result = await input.exit({
				expected: binding.expected(location),
				inject: async () =>
					requireDelivered(
						await input.input.mouse({
							kind: 'press',
							button: 'left',
							column: location.column,
							row: location.row,
						}),
					),
			});
			this.exited = {
				files: [this.bindings.receipt, join(this.bindings.target.evidence, 'terminal.ansi')],
				dispatchSequences: result.receipts.map((receipt) => receipt.sequence),
			};
			return this.exited;
		}
		return this.retain(
			await input.observe({
				expected: binding.expected(location),
				inject: async () => {
					const records = await input.input.click(location.column, location.row);
					for (const record of records) requireDelivered(record);
				},
			}),
		);
	}
	async focus(
		_step: Extract<FlowStep, { kind: 'focus' }>,
		index: number,
	): Promise<FlowStepEvidence> {
		const binding = requireBinding(this.bindings.inputs.get(index), 'focus step ' + index);
		if (binding.kind !== 'focus') throw new Error('expected a focus binding');
		const files: string[] = [],
			sequences: number[] = [];
		await binding.route.reduce(async (previous, key) => {
			await previous;
			if (binding.stopWhenFocused === true && binding.isFocused(this.context().frame.grid)) return;
			const evidence = this.retain(await this.context().input.press(key.key, key.expected));
			files.push(...evidence.files);
			sequences.push(...evidence.dispatchSequences);
		}, Promise.resolve());
		if (!binding.isFocused(this.context().frame.grid))
			throw new Error('declared focus route did not visibly focus its target');
		return { files, dispatchSequences: sequences };
	}
	async fixture(step: Extract<FlowStep, { kind: 'fixture' }>): Promise<FlowStepEvidence> {
		const result = await requireBinding(
			this.bindings.fixtures.get(step.operation),
			step.operation,
		)(this.context());
		return result === null ? { files: [], dispatchSequences: [] } : this.retain(result);
	}
	private async check(
		checks: readonly FlowCheck[],
		readback: unknown,
	): Promise<FlowCheckpointEvidence['checks']> {
		return Promise.all(
			checks.map(async (check) => ({
				check,
				failures: await requireBinding(this.bindings.checks.get(check), check)(
					this.context(),
					readback,
				),
			})),
		);
	}
	async capture(step: Extract<FlowStep, { kind: 'capture' }>): Promise<FlowCheckpointEvidence> {
		const barrier = this.bindings.checkpointBarriers?.get(step.checkpoint);
		if (barrier !== undefined) {
			await this.context().driver.waitForGrid(step.checkpoint + ' publication', barrier.ready);
			this.retain(await this.context().input.press(barrier.publish.key, barrier.publish.expected));
		}
		const { frame } = this.context();
		if (!/^[a-z0-9-]+$/u.test(step.checkpoint)) throw new Error('unsafe flow checkpoint name');
		const prefix = join(this.bindings.target.evidence, step.checkpoint);
		writeFileSync(prefix + '.ansi', frame.bytes);
		writeFileSync(prefix + '.grid.json', JSON.stringify(frame.grid, null, 2));
		writeFileSync(prefix + '.txt', gridLines(frame.grid).join('\n') + '\n');
		writeFileSync(
			prefix + '.frame.json',
			JSON.stringify({ binding: frame.binding, resizes: frame.resizes }, null, 2),
		);
		await renderOriginal({
			...this.bindings.original,
			output: this.bindings.target.evidence,
			checkpoint: step.checkpoint,
			bytes: frame.bytes,
			grid: frame.grid,
			resizes: frame.resizes,
			columns: frame.grid.columns,
			rows: frame.grid.rows,
		});
		const files = ['.ansi', '.grid.json', '.txt', '.frame.json', '.png', '.original.json'].map(
			(suffix) => prefix + suffix,
		);
		return {
			files,
			dispatchSequences: [frame.binding.sequence],
			checks: await this.check(step.checks, undefined),
		};
	}
	async readback(
		step: Extract<FlowStep, { kind: 'readback' }>,
		index: number,
	): Promise<FlowCheckpointEvidence> {
		// An ending input may have no frame, but its real typed effect is still readable.
		if (this.driver === null || this.input === null || this.frame === null)
			throw new Error('native flow did not start');
		const context = { driver: this.driver, input: this.input, frame: this.frame };
		const observed = await requireBinding(
			this.bindings.readbacks.get(step.port),
			step.port,
		)(context);
		const path = join(this.bindings.target.evidence, `readback-${index}-${step.port}.json`);
		writeFileSync(path, JSON.stringify(observed, null, 2));
		const checks = await Promise.all(
			step.checks.map(async (check) => ({
				check,
				failures: await requireBinding(this.bindings.checks.get(check), check)(context, observed),
			})),
		);
		return {
			files: [path],
			dispatchSequences: this.exited?.dispatchSequences ?? [this.frame.binding.sequence],
			checks,
		};
	}
	async finish(): Promise<FlowStepEvidence> {
		if (this.exited !== null) return this.exited;
		const { input } = this.context();
		const result = await input.exit({
			expected: this.bindings.quit.expected,
			inject: async () => requireDelivered(await input.input.key(this.bindings.quit.key)),
		});
		this.exited = {
			files: [this.bindings.receipt, join(this.bindings.target.evidence, 'terminal.ansi')],
			dispatchSequences: result.receipts.map((receipt) => receipt.sequence),
		};
		return this.exited;
	}
	async close(): Promise<void> {
		const failures: unknown[] = [];
		try {
			if (this.driver !== null) await this.driver.stop();
		} catch (error) {
			failures.push(error);
		}
		try {
			if (this.input !== null)
				writeFileSync(
					join(this.bindings.target.evidence, 'input-delivery.json'),
					JSON.stringify(this.input.input.history(), null, 2),
				);
		} catch (error) {
			failures.push(error);
		}
		if (failures.length > 0)
			throw new AggregateError(failures, 'native flow owned cleanup failed', {
				cause: failures[0],
			});
	}
}
export function nativeFlowCapabilities(bindings: NativeFlowBindings): FlowCapabilities {
	return {
		fixtures: [...bindings.fixtures.keys()],
		targets: [...bindings.targets.keys()],
		checks: [...bindings.checks.keys()],
		readbacks: [...bindings.readbacks.keys()],
		inputSteps: [...bindings.inputs.keys()],
	};
}
export function executeNativeFlow(
	invocation: FlowInvocation,
	bindings: NativeFlowBindings,
): Promise<FlowExecutionResult> {
	return executeFlow({
		invocation,
		capabilities: nativeFlowCapabilities(bindings),
		createRuntime: () => new NativeFlowRuntime(invocation, bindings),
		recordStep: (index, evidence) =>
			writeFileSync(
				join(bindings.target.evidence, `step-${index}.json`),
				JSON.stringify(evidence, null, 2),
			),
	});
}
