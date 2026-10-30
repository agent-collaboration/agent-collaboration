import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import {
	checkCodexAcp,
	codexAcpRunPlanSchema,
	codexChild,
	codexJourneyFor,
} from './codex-acp-run.ts';
import type { CodexAcpRunPlan } from './codex-acp-run.ts';
import { targetFixture } from './codex-acp-test-fixtures.ts';
import { initializedSourceRepository } from './controlled-child-fixtures.ts';
import { sourceBinding } from './source-binding.ts';

async function runPlan(journey: unknown): Promise<{ root: string; plan: CodexAcpRunPlan }> {
	const { root, target } = await targetFixture();
	const repository = initializedSourceRepository();
	const binding = target.codex;
	return {
		root,
		plan: codexAcpRunPlanSchema.parse({
			repository,
			source_head: sourceBinding(repository).head,
			driver: binding,
			node: binding,
			chrome: binding,
			keys: 'legacy',
			output: join(root, 'run'),
			target,
			journey,
		}),
	};
}
const CURSOR_SKILL = {
	kind: 'skill',
	provider: 'cursor',
	skills: ['bridge_probe_cursor_a_01a18013'],
	prefix: '',
	args: 'ALPHA',
	expected_effects: ['ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|RED'],
	image_path: null,
	effect_deadline_ms: 120_000,
};

test('each declared journey kind builds its scenes for the target provider only', async () => {
	const { plan } = await runPlan(CURSOR_SKILL);
	expect(codexJourneyFor(plan).name).toBe('codex-acp-j1-cursor-skill');
	const compact = await runPlan({
		kind: 'compact',
		provider: 'cursor',
		effect_deadline_ms: 120_000,
	});
	expect(codexJourneyFor(compact.plan).steps[0]?.checkpoint.scene).toBe('compact-refused');
	const interrupt = await runPlan({
		kind: 'interrupt',
		provider: 'cursor',
		prompt: 'count to fifty',
	});
	expect(codexJourneyFor(interrupt.plan).name).toBe('codex-acp-j3-cursor-interrupt');
	// The fixture target is the Cursor endpoint; a Claude journey must not drive it.
	const wrong = await runPlan({ ...CURSOR_SKILL, provider: 'claude' });
	expect(() => codexJourneyFor(wrong.plan)).toThrow(
		'journey provider differs from the target endpoint',
	);
});

test('the plan refuses relative output, unknown fields and unbounded effect waits', async () => {
	const { plan } = await runPlan(CURSOR_SKILL);
	expect(() => codexAcpRunPlanSchema.parse({ ...plan, output: 'run' })).toThrow();
	expect(() => codexAcpRunPlanSchema.parse({ ...plan, extra: true })).toThrow();
	expect(() =>
		codexAcpRunPlanSchema.parse({
			...plan,
			journey: { ...CURSOR_SKILL, effect_deadline_ms: 3_600_000 },
		}),
	).toThrow();
});

test('a changed source head is refused before any binary or Codex is touched', async () => {
	const { plan } = await runPlan(CURSOR_SKILL);
	await expect(checkCodexAcp({ ...plan, source_head: 'f'.repeat(40) })).rejects.toThrow(
		'declared source head changed',
	);
});

test('a PTY child that is not the declared Codex, or is gone, is refused', async () => {
	const { root, plan } = await runPlan(CURSOR_SKILL);
	const output = join(root, 'pty');
	mkdirSync(output);
	// This test process is a live child owned by this user, but it is Node, not the declared Codex.
	writeFileSync(join(output, 'quota-child.pid'), String(process.pid));
	expect(() => codexChild(output, plan.target)).toThrow('not the declared installed Codex');
	// A real child that has already exited and been reaped.
	const exited = spawnSync(process.execPath, ['-e', '']);
	writeFileSync(join(output, 'quota-child.pid'), String(exited.pid));
	expect(() => codexChild(output, plan.target)).toThrow('not the declared installed Codex');
});
