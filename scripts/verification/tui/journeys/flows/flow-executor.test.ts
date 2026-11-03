import { expect, test, vi } from 'vitest';

import type { FlowCheck, FlowJourney, FlowStep } from './flow-definition.ts';
import { absentAtBase, flowReference } from './flow-definition.ts';
import { admitFlow, executeFlow } from './flow-executor.ts';
import type {
	FlowCapabilities,
	FlowCheckpointEvidence,
	FlowRuntime,
	FlowStepEvidence,
} from './flow-executor.ts';

const journey: FlowJourney = {
	journeyId: 'controlled-flow',
	requirements: ['U12/R13'],
	surface: 'sessions',
	availability: absentAtBase('S6'),
	reference: flowReference('R13', 'U12'),
	grids: ['120x36', '32x24'],
	keys: ['legacy', 'kitty'],
	fixtures: ['controlled fixture'],
	steps: [
		{
			kind: 'fixture',
			operation: 'seed-case',
			purpose: 'controlled admission, not a product fixture',
		},
		{ kind: 'press', key: 'F10' },
		{ kind: 'capture', checkpoint: 'menu', checks: ['closed-menu-inventory', 'projection-parity'] },
		{ kind: 'click', target: 'Help' },
		{ kind: 'focus', target: 'Close', via: 'Tab' },
		{ kind: 'type', text: 'query' },
		{ kind: 'readback', port: 'clipboard', checks: ['clipboard-full-account-id'] },
	],
	wrongOutcomes: ['missing check accepted', 'an unbound fixture launches a process'],
	admissionInputs: [],
};
const capabilities: FlowCapabilities = {
	fixtures: ['seed-case'],
	targets: ['Help', 'Close'],
	checks: ['closed-menu-inventory', 'projection-parity', 'clipboard-full-account-id'],
	readbacks: ['clipboard'],
};
const evidence: FlowStepEvidence = { files: ['retained-evidence'], dispatchSequences: [1] };
class ControlledRuntime implements FlowRuntime {
	readonly calls: string[] = [];
	failCheck: FlowCheck | null = null;
	omitCheck = false;
	failStart = false;
	failClose = false;
	async start(): Promise<void> {
		this.calls.push('start');
		if (this.failStart) throw new Error('start refused');
	}
	private action(step: FlowStep): FlowStepEvidence {
		this.calls.push(step.kind);
		return evidence;
	}
	async press(step: Extract<FlowStep, { kind: 'press' }>): Promise<FlowStepEvidence> {
		return this.action(step);
	}
	async type(step: Extract<FlowStep, { kind: 'type' }>): Promise<FlowStepEvidence> {
		return this.action(step);
	}
	async click(step: Extract<FlowStep, { kind: 'click' }>): Promise<FlowStepEvidence> {
		return this.action(step);
	}
	async focus(step: Extract<FlowStep, { kind: 'focus' }>): Promise<FlowStepEvidence> {
		return this.action(step);
	}
	async fixture(step: Extract<FlowStep, { kind: 'fixture' }>): Promise<FlowStepEvidence> {
		return this.action(step);
	}
	private checkpoint(
		step: Extract<FlowStep, { kind: 'capture' | 'readback' }>,
	): FlowCheckpointEvidence {
		this.calls.push(step.kind);
		const checks = step.checks.map((check) => ({
			check,
			failures: this.failCheck === check ? ['wrong observed target'] : [],
		}));
		return { ...evidence, checks: this.omitCheck ? checks.slice(1) : checks };
	}
	async capture(step: Extract<FlowStep, { kind: 'capture' }>): Promise<FlowCheckpointEvidence> {
		return this.checkpoint(step);
	}
	async readback(step: Extract<FlowStep, { kind: 'readback' }>): Promise<FlowCheckpointEvidence> {
		return this.checkpoint(step);
	}
	async finish(): Promise<FlowStepEvidence> {
		this.calls.push('finish');
		return evidence;
	}
	async close(): Promise<void> {
		this.calls.push('close');
		if (this.failClose) throw new Error('owned cleanup refused');
	}
}
test('all missing bindings are reported before constructing any process-owning runtime', async () => {
	const createRuntime = vi.fn(() => new ControlledRuntime());
	const result = await executeFlow({
		invocation: { journey, grid: '120x36', keys: 'legacy' },
		capabilities: { fixtures: [], targets: [], checks: [], readbacks: [] },
		createRuntime,
		recordStep: () => {
			throw new Error('unadmitted step executed');
		},
	});
	expect(result.kind).toBe('fixture-gap');
	if (result.kind !== 'fixture-gap') throw new Error('missing bindings admitted');
	expect(result.gaps).toContainEqual({ kind: 'fixture', name: 'seed-case' });
	expect(result.gaps).toContainEqual({ kind: 'target', name: 'Help' });
	expect(result.gaps).toContainEqual({ kind: 'check', name: 'projection-parity' });
	expect(result.gaps).toContainEqual({ kind: 'readback', name: 'clipboard' });
	expect(createRuntime).not.toHaveBeenCalled();
});
test('unsupported size/key mode and a late missing fixture cannot partially start the flow', () => {
	const late: FlowJourney = {
		...journey,
		steps: [
			...journey.steps,
			{ kind: 'fixture', operation: 'late-unbound', purpose: 'must reject before startup' },
		],
	};
	expect(admitFlow({ journey: late, grid: '76x30', keys: 'kitty' }, capabilities)).toContainEqual({
		kind: 'grid',
		name: '76x30',
	});
	expect(
		admitFlow(
			{ journey: { ...late, keys: ['legacy'] }, grid: '120x36', keys: 'kitty' },
			capabilities,
		),
	).toEqual([
		{ kind: 'keys', name: 'kitty' },
		{ kind: 'fixture', name: 'late-unbound' },
	]);
});
test('ordered authored steps finish and close the retained runtime once', async () => {
	const runtime = new ControlledRuntime();
	const recorded: number[] = [];
	const result = await executeFlow({
		invocation: { journey, grid: '32x24', keys: 'kitty' },
		capabilities,
		createRuntime: () => runtime,
		recordStep: (index) => {
			recorded.push(index);
		},
	});
	expect(result.kind).toBe('completed');
	expect(recorded).toEqual([0, 1, 2, 3, 4, 5, 6]);
	expect(runtime.calls).toEqual([
		'start',
		'fixture',
		'press',
		'capture',
		'click',
		'focus',
		'type',
		'readback',
		'finish',
		'close',
	]);
});
test.each(['failed', 'omitted'] as const)(
	'a %s observation stops later effects while keeping owned cleanup',
	async (kind) => {
		const runtime = new ControlledRuntime();
		if (kind === 'failed') runtime.failCheck = 'projection-parity';
		else runtime.omitCheck = true;
		await expect(
			executeFlow({
				invocation: { journey, grid: '120x36', keys: 'legacy' },
				capabilities,
				createRuntime: () => runtime,
				recordStep: () => {},
			}),
		).rejects.toThrow(kind === 'failed' ? 'wrong observed target' : 'omitted, duplicated');
		expect(runtime.calls).toEqual(['start', 'fixture', 'press', 'capture', 'close']);
	},
);
test('start and cleanup failures are both retained without executing an input', async () => {
	const runtime = new ControlledRuntime();
	runtime.failStart = true;
	runtime.failClose = true;
	try {
		await executeFlow({
			invocation: { journey, grid: '120x36', keys: 'legacy' },
			capabilities,
			createRuntime: () => runtime,
			recordStep: () => {},
		});
		throw new Error('expected startup failure');
	} catch (error) {
		expect(error).toBeInstanceOf(AggregateError);
		if (!(error instanceof AggregateError)) throw error;
		expect(
			error.errors.map((entry: unknown) =>
				entry instanceof Error ? entry.message : String(entry),
			),
		).toEqual(['start refused', 'owned cleanup refused']);
	}
	expect(runtime.calls).toEqual(['start', 'close']);
});
