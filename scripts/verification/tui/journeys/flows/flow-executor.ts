/** Execute authored data only after every required fixture, target and oracle is bound. */
import type { KeyProtocol } from '../../terminal/pty-grid-driver.ts';
import type { JourneyGridSize } from '../journey-definition.ts';
import type { FlowCheck, FlowJourney, FlowStep } from './flow-definition.ts';
import { flowJourneySchema } from './flow-definition.ts';

export interface FlowCapabilities {
	readonly fixtures: readonly string[];
	readonly targets: readonly string[];
	readonly checks: readonly FlowCheck[];
	readonly readbacks: readonly Extract<FlowStep, { kind: 'readback' }>['port'][];
	/** Native adapters must declare each authored input's independent ACK binding. */
	readonly inputSteps?: readonly number[];
}
export interface FlowInvocation {
	readonly journey: FlowJourney;
	readonly grid: JourneyGridSize;
	readonly keys: KeyProtocol;
}
export interface FlowAdmissionGap {
	readonly kind: 'grid' | 'keys' | 'fixture' | 'target' | 'check' | 'readback' | 'input';
	readonly name: string;
}
export interface FlowCheckResult {
	readonly check: FlowCheck;
	readonly failures: readonly string[];
}
export interface FlowStepEvidence {
	readonly files: readonly string[];
	readonly dispatchSequences: readonly number[];
	readonly inputObservation?: {
		readonly kind: 'after-input' | 'after-burst';
		readonly sequence: number;
	};
}
export interface FlowCheckpointEvidence extends FlowStepEvidence {
	readonly checks: readonly FlowCheckResult[];
}
/** The fixture adapter owns semantic selectors and real producer readbacks; no stand-in passes. */
export interface FlowRuntime {
	start(): Promise<void>;
	press(step: Extract<FlowStep, { kind: 'press' }>, index: number): Promise<FlowStepEvidence>;
	type(step: Extract<FlowStep, { kind: 'type' }>, index: number): Promise<FlowStepEvidence>;
	click(step: Extract<FlowStep, { kind: 'click' }>, index: number): Promise<FlowStepEvidence>;
	focus(step: Extract<FlowStep, { kind: 'focus' }>, index: number): Promise<FlowStepEvidence>;
	fixture(step: Extract<FlowStep, { kind: 'fixture' }>, index: number): Promise<FlowStepEvidence>;
	capture(
		step: Extract<FlowStep, { kind: 'capture' }>,
		index: number,
	): Promise<FlowCheckpointEvidence>;
	readback(
		step: Extract<FlowStep, { kind: 'readback' }>,
		index: number,
	): Promise<FlowCheckpointEvidence>;
	finish(): Promise<FlowStepEvidence>;
	close(): Promise<void>;
}
export type FlowExecutionResult =
	| { readonly kind: 'fixture-gap'; readonly gaps: readonly FlowAdmissionGap[] }
	| {
			readonly kind: 'completed';
			readonly steps: readonly FlowStepEvidence[];
			readonly finish: FlowStepEvidence;
	  };

/** Inspect the whole journey before constructing a process-owning runtime. */
export function admitFlow(
	invocation: FlowInvocation,
	available: FlowCapabilities,
): readonly FlowAdmissionGap[] {
	const journey = flowJourneySchema.parse(invocation.journey);
	const gaps: FlowAdmissionGap[] = [];
	if (!journey.grids.includes(invocation.grid)) gaps.push({ kind: 'grid', name: invocation.grid });
	if (!journey.keys.includes(invocation.keys)) gaps.push({ kind: 'keys', name: invocation.keys });
	for (const [index, step] of journey.steps.entries()) {
		if (
			available.inputSteps !== undefined &&
			['press', 'type', 'click', 'focus'].includes(step.kind) &&
			!available.inputSteps.includes(index)
		)
			gaps.push({ kind: 'input', name: 'step ' + index });
		switch (step.kind) {
			case 'fixture':
				if (!available.fixtures.includes(step.operation))
					gaps.push({ kind: 'fixture', name: step.operation });
				break;
			case 'click':
			case 'focus':
				if (!available.targets.includes(step.target))
					gaps.push({ kind: 'target', name: step.target });
				break;
			case 'readback':
				if (!available.readbacks.includes(step.port))
					gaps.push({ kind: 'readback', name: step.port });
				for (const check of step.checks)
					if (!available.checks.includes(check)) gaps.push({ kind: 'check', name: check });
				break;
			case 'capture':
				for (const check of step.checks)
					if (!available.checks.includes(check)) gaps.push({ kind: 'check', name: check });
				break;
			case 'press':
			case 'type':
				break;
		}
	}
	return gaps.filter(
		(gap, index) =>
			gaps.findIndex((other) => other.kind === gap.kind && other.name === gap.name) === index,
	);
}
function requireAllChecks(expected: readonly FlowCheck[], observed: FlowCheckpointEvidence): void {
	if (JSON.stringify(observed.checks.map((result) => result.check)) !== JSON.stringify(expected))
		throw new Error('flow checkpoint omitted, duplicated or changed its declared checks');
	const failures = observed.checks.flatMap((result) =>
		result.failures.map((message) => result.check + ': ' + message),
	);
	if (failures.length > 0) throw new Error('flow checkpoint failed: ' + failures.join('; '));
}
function performStep(
	runtime: FlowRuntime,
	step: FlowStep,
	index: number,
): Promise<FlowStepEvidence> {
	switch (step.kind) {
		case 'press':
			return runtime.press(step, index);
		case 'type':
			return runtime.type(step, index);
		case 'click':
			return runtime.click(step, index);
		case 'focus':
			return runtime.focus(step, index);
		case 'fixture':
			return runtime.fixture(step, index);
		case 'capture':
			return runtime.capture(step, index).then((observed) => {
				requireAllChecks(step.checks, observed);
				return observed;
			});
		case 'readback':
			return runtime.readback(step, index).then((observed) => {
				requireAllChecks(step.checks, observed);
				return observed;
			});
	}
	const unreachable: never = step;
	throw new Error('unknown flow step: ' + String(unreachable));
}
export async function executeFlow(options: {
	readonly invocation: FlowInvocation;
	readonly capabilities: FlowCapabilities;
	readonly createRuntime: () => FlowRuntime;
	readonly recordStep: (index: number, evidence: FlowStepEvidence) => void;
}): Promise<FlowExecutionResult> {
	const gaps = admitFlow(options.invocation, options.capabilities);
	if (gaps.length > 0) return { kind: 'fixture-gap', gaps };
	const runtime = options.createRuntime();
	let settlement:
		| { readonly kind: 'completed'; readonly result: FlowExecutionResult }
		| { readonly kind: 'failed'; readonly error: unknown };
	try {
		await runtime.start();
		const steps: FlowStepEvidence[] = [];
		await options.invocation.journey.steps.reduce(async (previous, step, index) => {
			await previous;
			const evidence = await performStep(runtime, step, index);
			steps.push(evidence);
			options.recordStep(index, evidence);
		}, Promise.resolve());
		settlement = {
			kind: 'completed',
			result: { kind: 'completed', steps, finish: await runtime.finish() },
		};
	} catch (error) {
		settlement = { kind: 'failed', error };
	}
	try {
		await runtime.close();
	} catch (cleanupError) {
		if (settlement.kind === 'failed')
			throw new AggregateError(
				[settlement.error, cleanupError],
				'flow failed and owned cleanup failed',
				{ cause: cleanupError },
			);
		throw cleanupError;
	}
	if (settlement.kind === 'failed') throw settlement.error;
	return settlement.result;
}
