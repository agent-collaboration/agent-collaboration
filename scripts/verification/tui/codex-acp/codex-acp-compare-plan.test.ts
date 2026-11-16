import { join } from 'node:path';

import { expect, test } from 'vitest';

import { sourceBinding } from '../evidence/source-binding.ts';
import { initializedSourceRepository } from '../fixtures/controlled-child-fixtures.ts';
import { compareJourneySchema } from './codex-acp-compare-scenes.ts';
import {
	codexAcpRunPlanSchema,
	codexJourneyFor,
	compareReadsProviderUsage,
	compareRunRecord,
} from './codex-acp-run.ts';
import type { CodexAcpRunPlan } from './codex-acp-run.ts';
import { targetFixture } from './codex-acp-test-fixtures.ts';

const ROUTER_SELECTION = {
	model: 'composer-2',
	selectOnCreate: false,
	selectOnRestore: false,
	requiredOptions: {},
};

const COMPARE = {
	kind: 'compare',
	provider: 'codex',
	send: { prompt: 'Reply with only the product of 4321 and 5678', completion: 'ANSWER 24534638' },
	steer: {
		prompt: 'Count from 1 to 300, one number per line, then stop',
		steer_text: 'Stop counting and reply with only the word hyacinth',
		completion: 'STEERED hyacinth',
	},
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
	scripted_settings: { model: 'gpt-5.5-mini', effort: 'low', service_tier: null },
	initial_selection: null,
	status: { footer_cells: ['gpt-5.5-mini low', '258K window'] },
	reply_deadline_ms: 120_000,
} as const;

async function planFor(
	endpoint: 'cursor-local' | 'codex-local',
	journey: unknown,
	targetPatch: (root: string) => Record<string, unknown> = () => ({}),
	planPatch: Record<string, unknown> = {},
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
			status_line: ['model-with-reasoning', 'context-window-size'],
			wire_tee: { listen_socket: join(root, 'tee.sock'), recording: join(output, 'wire.jsonl') },
			...targetPatch(root),
		},
		journey,
		...planPatch,
	});
}

test('a compare plan on the real-Codex endpoint builds the script and records a reference run', async () => {
	const plan = await planFor('codex-local', COMPARE);
	expect(codexJourneyFor(plan).name).toBe('codex-acp-o5-codex-compare');
	expect(compareReadsProviderUsage(plan)).toBe(false);
	expect(compareRunRecord(plan)).toEqual({
		run_kind: 'codex-reference',
		wire_recording: join(plan.output, 'wire.jsonl'),
		scripted_settings: COMPARE.scripted_settings,
		agent_approval_outcomes: null,
	});
});

test('the same script on a Router endpoint is a candidate run that reads the provider usage', async () => {
	const plan = await planFor('cursor-local', {
		...COMPARE,
		provider: 'cursor',
		initial_selection: ROUTER_SELECTION,
	});
	expect(codexJourneyFor(plan).steps.map((step) => step.checkpoint.scene)).toEqual(
		(await planFor('codex-local', COMPARE).then(codexJourneyFor)).steps.map(
			(step) => step.checkpoint.scene,
		),
	);
	expect(compareReadsProviderUsage(plan)).toBe(true);
	expect(compareRunRecord(plan).run_kind).toBe('router-candidate');
});

test('each endpoint maps to exactly one journey provider', async () => {
	await expect(
		planFor('codex-local', {
			...COMPARE,
			provider: 'cursor',
			initial_selection: ROUTER_SELECTION,
		}).then(codexJourneyFor),
	).rejects.toThrow('journey provider differs from the target endpoint');
	await expect(planFor('cursor-local', COMPARE).then(codexJourneyFor)).rejects.toThrow(
		'journey provider differs from the target endpoint',
	);
	// A Claude journey on the real-Codex endpoint is refused too.
	await expect(
		planFor('codex-local', { kind: 'picker', provider: 'claude', skill: 'some_skill' }).then(
			codexJourneyFor,
		),
	).rejects.toThrow('journey provider differs from the target endpoint');
});

test('only the comparison journey admits real Codex as its provider', async () => {
	await expect(
		planFor('codex-local', { kind: 'picker', provider: 'codex', skill: 'some_skill' }),
	).rejects.toThrow();
	expect(() => compareJourneySchema.parse({ ...COMPARE, provider: 'gemini' })).toThrow();
});

test('a comparison run needs the wire tee and a footer with the model and the window', async () => {
	await expect(planFor('codex-local', COMPARE, () => ({ wire_tee: null }))).rejects.toThrow(
		'records the wire',
	);
	await expect(
		planFor('codex-local', COMPARE, () => ({ status_line: ['model-with-reasoning'] })),
	).rejects.toThrow('model and context window');
});

test('the tee recording must lie under the run output', async () => {
	await expect(
		planFor('codex-local', COMPARE, (root) => ({
			wire_tee: { listen_socket: join(root, 'tee.sock'), recording: join(root, 'elsewhere.jsonl') },
		})),
	).rejects.toThrow('belongs under the run output');
});

test('a Router comparison target declares its initial selection; the real-Codex reference does not', async () => {
	await expect(planFor('cursor-local', { ...COMPARE, provider: 'cursor' })).rejects.toThrow(
		'a Router comparison target declares its initial selection',
	);
	await expect(
		planFor('codex-local', { ...COMPARE, initial_selection: ROUTER_SELECTION }),
	).rejects.toThrow('a Router comparison target declares its initial selection');
});

const NAMED_RED = {
	scene: 'model-applied',
	reason: 'thread/settings/update with serviceTier refused, -32000; fixed by da58f7bec (#242)',
	error_line: 'Failed to update thread settings: thread/settings/update failed in TUI',
};

test('only a baseline comparison run may name historical reds; a head run stays strict', async () => {
	const head = await planFor('codex-local', COMPARE);
	expect(head.build).toBe('head');
	expect(head.historical_reds).toEqual([]);
	await expect(
		planFor('codex-local', COMPARE, undefined, { historical_reds: [NAMED_RED] }),
	).rejects.toThrow('a head run stays strict');
	const baseline = await planFor('codex-local', COMPARE, undefined, {
		build: 'baseline',
		historical_reds: [NAMED_RED],
	});
	expect(baseline.historical_reds).toEqual([NAMED_RED]);
});
