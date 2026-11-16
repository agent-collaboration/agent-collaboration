import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { sourceBinding } from '../evidence/source-binding.ts';
import { initializedSourceRepository } from '../fixtures/controlled-child-fixtures.ts';
import {
	approvalRunRecord,
	movedRunRecord,
	requireApprovalScratchBinding,
	requireFreshOutput,
} from './codex-acp-moved-run.ts';
import { codexAcpRunPlanSchema, codexJourneyFor } from './codex-acp-run.ts';
import type { CodexAcpRunPlan } from './codex-acp-run.ts';
import {
	ALIAS,
	LAUNCHED_THREAD,
	referenceStartRecording,
	targetFixture,
} from './codex-acp-test-fixtures.ts';

const OTHER_THREAD = '019a0000-0000-7000-8000-0000000000b2';
const RESUME = {
	kind: 'resume',
	provider: 'codex',
	command_row: 'DISCOVERY-resume-command-row',
	picker: {
		header: 'DISCOVERY-picker-header',
		rows: ['DISCOVERY-launch-row', 'DISCOVERY-other-row'],
		navigation: [{ kind: 'key', key: 'Down' }],
		target_row: 'DISCOVERY-other-row',
	},
	resumed: {
		history_markers: ['DISCOVERY-other-history'],
		other_thread_markers: ['DISCOVERY-launch-history'],
	},
	expected_thread_id: OTHER_THREAD,
	deadline_ms: 30_000,
} as const;
const REFUSED = {
	kind: 'refused',
	provider: 'codex',
	commands: [{ command: '/fork', refusal: 'DISCOVERY-fork-refusal' }],
} as const;
const TIER = {
	kind: 'tier',
	provider: 'codex',
	model: {
		catalogue_page: ['gpt-5.5', 'gpt-5.5-mini'],
		stale_rows: [],
		model_navigation: [{ kind: 'key', key: 'Down' }],
		target: { display_name: 'gpt-5.5-mini', number: 2, is_current: false },
		effort: null,
		apply: 'session',
		expected_outcome: ['Model changed to gpt-5.5-mini'],
		outcome_deadline_ms: 20_000,
	},
	tier: {
		open: [{ kind: 'key', key: 'Tab' }],
		popup_header: 'DISCOVERY-tier-header',
		navigation: [],
		row: { number: 2, label: 'DISCOVERY-tier-row', is_current: false, is_default: false },
		apply: { kind: 'key', key: 'Enter' },
		outcome: ['DISCOVERY-tier-applied'],
		outcome_deadline_ms: 20_000,
		initial_wire_value: null,
		wire_value: 'priority',
	},
	scripted_settings: { model: 'gpt-5.5-mini', effort: 'low', service_tier: 'priority' },
} as const;
const approvalJourney = (scratch: string, answer: unknown): Record<string, unknown> => ({
	kind: 'approval',
	provider: 'claude',
	session_mode: 'default',
	action_kind: 'command',
	window_b: { result_text: 'DISCOVERY-b-result' },
	scratch_directory: scratch,
	file_name: 'approval-probe.txt',
	trigger: {
		prompt: 'Create approval-probe.txt here, then reply with only the product of 4321 and 5678',
		completion: 'ANSWER 24534638',
	},
	prompt_shown: { prompt_line: 'DISCOVERY-prompt', options: [], deadline_ms: 60_000 },
	answer,
	reply_deadline_ms: 120_000,
});
const ALLOW = {
	case: 'allow-once',
	actions: [{ kind: 'key', key: 'Enter' }],
	cli_decision: 'DISCOVERY-accept',
	history_cell: 'DISCOVERY-approved',
	deadline_ms: 20_000,
};
const TIMEOUT = {
	case: 'timeout',
	actions: [],
	timeout_ms: 60_000,
	history_cell: 'DISCOVERY-timed-out',
	deadline_ms: 90_000,
};

async function planFor(
	endpoint: 'cursor-local' | 'codex-local',
	journey: (output: string) => unknown,
	patch: (root: string) => Record<string, unknown> = () => ({}),
): Promise<CodexAcpRunPlan> {
	const { root, target } = await targetFixture(false, endpoint);
	const repository = initializedSourceRepository();
	const output = join(root, 'run');
	return codexAcpRunPlanSchema.parse({
		repository,
		source_head: sourceBinding(repository).head,
		driver: target.codex,
		node: target.codex,
		chrome: target.codex,
		keys: 'legacy',
		output,
		target: {
			...target,
			wire_tee: { listen_socket: join(root, 'tee.sock'), recording: join(output, 'wire.jsonl') },
			...patch(root),
		},
		journey: journey(output),
	});
}
const claude = { endpoint_id: 'claude-local' };

test('resume, refused and tier plans on the real-Codex endpoint build their journeys', async () => {
	const cases = [
		[RESUME, 'codex-acp-o5-codex-resume'],
		[REFUSED, 'codex-acp-o5-codex-refused'],
		[TIER, 'codex-acp-o5-codex-tier'],
	] as const;
	const names = await Promise.all(
		cases.map(
			async ([journey]) => codexJourneyFor(await planFor('codex-local', () => journey)).name,
		),
	);
	expect(names).toEqual(cases.map(([, name]) => name));
});

test('each moved journey leaves the run record the wire evaluation reads', async () => {
	const resume = await planFor('codex-local', () => RESUME);
	// The real-Codex reference launches on the thread its own thread/start reported on the wire.
	mkdirSync(resume.output, { recursive: true });
	writeFileSync(join(resume.output, 'wire.jsonl'), referenceStartRecording(LAUNCHED_THREAD));
	expect(movedRunRecord(resume)).toEqual({
		kind: 'resume',
		wire_recording: join(resume.output, 'wire.jsonl'),
		launch_thread_id: LAUNCHED_THREAD,
		expected_thread_id: OTHER_THREAD,
	});
	expect(movedRunRecord(await planFor('codex-local', () => TIER))).toMatchObject({
		kind: 'tier',
		wire_value: 'priority',
	});
	expect(movedRunRecord(await planFor('codex-local', () => REFUSED))).toBeNull();
	const allow = await planFor(
		'cursor-local',
		(output) => approvalJourney(join(output, 'scratch'), ALLOW),
		() => claude,
	);
	// Approval runs have two windows, so their record is the approval comparison's, not the moved one.
	expect(movedRunRecord(allow)).toBeNull();
	expect(approvalRunRecord(allow)).toMatchObject({
		run_kind: 'router-candidate',
		window_a: 0,
		window_b: 1,
		action_kind: 'command',
		cli_choice: 'DISCOVERY-accept',
	});
	const timeout = await planFor(
		'cursor-local',
		(output) => approvalJourney(join(output, 'scratch'), TIMEOUT),
		() => claude,
	);
	expect(approvalRunRecord(timeout)).toMatchObject({ cli_choice: null });
});

test('wrong pairings are refused: no tee, resuming the launch thread, tier without /model', async () => {
	await expect(
		planFor(
			'codex-local',
			() => RESUME,
			() => ({ wire_tee: null }),
		),
	).rejects.toThrow('records the wire');
	// The real-Codex reference's launch thread is known only from its own thread/start, so the
	// moved evaluation (resumed thread vs launch thread) refuses a resume of it after the run; a
	// Router target is refused here, from its declared Session.
	await expect(
		planFor('cursor-local', () => ({ ...RESUME, provider: 'cursor', expected_thread_id: ALIAS })),
	).rejects.toThrow('another thread than the one the CLI launched on');
	const { model: _model, ...withoutModel } = TIER;
	await expect(planFor('codex-local', () => withoutModel)).rejects.toThrow();
	await expect(
		planFor('codex-local', () => ({ ...TIER, model: { ...TIER.model, apply: 'default' } })),
	).rejects.toThrow();
});

test('an approval plan is refused on Cursor, and a provider that does not match its endpoint', async () => {
	await expect(
		planFor('cursor-local', (output) => approvalJourney(join(output, 'scratch'), ALLOW)),
	).rejects.toThrow('Claude Router Session or the real-Codex reference');
	// A Claude journey on the real-Codex endpoint, and a codex journey on a Claude one.
	await expect(
		planFor('codex-local', (output) => approvalJourney(join(output, 'scratch'), ALLOW)).then(
			codexJourneyFor,
		),
	).rejects.toThrow('journey provider differs from the target endpoint');
});

const codexApproval = (output: string, answer: unknown): Record<string, unknown> => ({
	...approvalJourney(join(output, 'scratch'), answer),
	provider: 'codex',
	session_mode: null,
});

test('the approval reference runs on real Codex, with no Router session mode and no timeout', async () => {
	const plan = await planFor(
		'codex-local',
		(output) => codexApproval(output, ALLOW),
		(root) => ({ cwd: join(root, 'run', 'scratch') }),
	);
	expect(codexJourneyFor(plan).name).toBe('codex-acp-o5-codex-approval-allow-once');
	expect(approvalRunRecord(plan)).toMatchObject({
		run_kind: 'codex-reference',
		cli_choice: 'DISCOVERY-accept',
	});
	await expect(planFor('codex-local', (output) => codexApproval(output, TIMEOUT))).rejects.toThrow(
		'real Codex has no approval timeout',
	);
	await expect(
		planFor('codex-local', (output) => ({
			...codexApproval(output, ALLOW),
			session_mode: 'default',
		})),
	).rejects.toThrow('declares its Session mode');
});

test('the real-Codex reference launches in its scratch directory', async () => {
	const inside = await planFor(
		'codex-local',
		(output) => codexApproval(output, ALLOW),
		(root) => ({ cwd: join(root, 'run', 'scratch') }),
	);
	expect(() => requireApprovalScratchBinding(inside)).not.toThrow();
	const elsewhere = await planFor('codex-local', (output) => codexApproval(output, ALLOW));
	expect(() => requireApprovalScratchBinding(elsewhere)).toThrow(
		'launch directory is not the declared scratch',
	);
});

test('an approval scratch directory outside the run output is refused', async () => {
	await expect(
		planFor(
			'cursor-local',
			() => approvalJourney('/tmp/elsewhere/scratch', ALLOW),
			() => claude,
		),
	).rejects.toThrow('scratch directory lies inside the run output');
});

test('an approval journey builds on a Claude endpoint', async () => {
	const plan = await planFor(
		'cursor-local',
		(output) => approvalJourney(join(output, 'scratch'), ALLOW),
		() => claude,
	);
	expect(codexJourneyFor(plan).name).toBe('codex-acp-o5-claude-approval-allow-once');
});

test('the approval scratch directory is the Session cwd the observer plan declares', async () => {
	const plan = await planFor(
		'cursor-local',
		(output) => approvalJourney(join(output, 'scratch'), ALLOW),
		() => claude,
	);
	const observerPath = plan.target.observer_plan;
	if (observerPath === null || plan.journey.kind !== 'approval') throw new Error('fixture');
	writeFileSync(observerPath, JSON.stringify({ cwd: plan.journey.scratch_directory }));
	expect(() => requireApprovalScratchBinding(plan)).not.toThrow();
	writeFileSync(observerPath, JSON.stringify({ cwd: '/somewhere/else' }));
	expect(() => requireApprovalScratchBinding(plan)).toThrow(
		'Session cwd is not the declared scratch',
	);
});

test('the run output is new, except that an approval run may hold its scratch directory alone', async () => {
	const plan = await planFor(
		'cursor-local',
		(output) => approvalJourney(join(output, 'scratch'), ALLOW),
		() => claude,
	);
	expect(() => requireFreshOutput(plan)).not.toThrow();
	mkdirSync(join(plan.output, 'scratch'), { recursive: true });
	expect(() => requireFreshOutput(plan)).not.toThrow();
	writeFileSync(join(plan.output, 'old-run.json'), '{}');
	expect(() => requireFreshOutput(plan)).toThrow('fresh Codex ACP output required');
	const resume = await planFor('codex-local', () => RESUME);
	mkdirSync(resume.output);
	expect(() => requireFreshOutput(resume)).toThrow('fresh Codex ACP output required');
});
