import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import {
	checkCodexAcp,
	codexAcpRunPlanSchema,
	codexChild,
	codexJourneyFor,
	exitCodex,
} from './codex-acp-run.ts';
import type { CodexAcpRunPlan } from './codex-acp-run.ts';
import { targetFixture } from './codex-acp-test-fixtures.ts';
import { controlledExecutable, initializedSourceRepository } from './controlled-child-fixtures.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import { temporaryRoot } from './receipt-fixtures.ts';
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

// A stand-in for the drive_terminal line protocol that keeps Codex 0.165's composer rule for
// typed text (chat_composer.rs handle_input_basic, paste_burst.rs): text written as one burst is
// buffered until the next frame is drawn, and an Enter that arrives first becomes a newline, so
// the PTY never ends. Enter after the drawn `/quit` row dispatches it and the PTY ends.
const COMPOSER_STANDIN = `
const { appendFileSync } = require('node:fs');
let state = 'idle';
const frame = Buffer.from('\\x1b[?2026h\\x1b[2J\\x1b[H› /quit\\r\\n  /quit     exit Codex\\x1b[?2026l').toString('hex');
process.stdout.write('TUI_GRID READY 120 36\\n');
let pending = '';
process.stdin.on('data', (chunk) => {
	pending += chunk.toString('utf8');
	while (pending.includes('\\n')) {
		const line = pending.slice(0, pending.indexOf('\\n'));
		pending = pending.slice(pending.indexOf('\\n') + 1);
		appendFileSync(process.env.TUI_STANDIN_LOG, line + '\\n');
		const [command, argument] = line.split(' ');
		const bytes = command === 'write' ? Buffer.from(argument, 'hex').toString('utf8') : '';
		let reply = 'OK';
		if (command === 'write' && bytes === '/quit') state = 'buffered';
		else if (command === 'write' && bytes === '\\r') state = state === 'drawn' ? 'exiting' : 'newline';
		else if (command === 'read') {
			if (state === 'buffered') { state = 'drawn'; reply = 'BYTES ' + frame; }
			else if (state === 'exiting') reply = 'EOF';
			else reply = 'READ_ERROR timed out waiting on channel';
		} else if (command === 'finish' || command === 'stop') {
			process.stdout.write('TUI_GRID FINISHED 0\\n');
			process.exit(0);
		}
		process.stdout.write('TUI_GRID ' + reply + '\\n');
	}
});
`;

async function composerStandIn(): Promise<{ driver: PtyGridDriver; log: () => string[] }> {
	const root = temporaryRoot();
	const script = controlledExecutable(root, 'drive-terminal', COMPOSER_STANDIN);
	writeFileSync(join(root, 'commands.log'), '');
	const driver = await PtyGridDriver.start({
		repository: root,
		driver: script,
		surface: 'codex-acp',
		keys: 'legacy',
		evidence: root,
		publicEnvironment: { TUI_STANDIN_LOG: join(root, 'commands.log') },
	});
	return {
		driver,
		log: () => readFileSync(join(root, 'commands.log'), 'utf8').trim().split('\n'),
	};
}

test('Codex exits normally only when Enter follows the drawn /quit row', async () => {
	const { driver, log } = await composerStandIn();
	try {
		await exitCodex(driver, 'legacy');
		const commands = log();
		const typed = commands.indexOf(`write ${Buffer.from('/quit').toString('hex')}`);
		const enter = commands.indexOf('write 0d');
		expect(typed).toBeGreaterThanOrEqual(0);
		// At least one read (the drawn frame) separates the typed command from its Enter.
		expect(commands.slice(typed + 1, enter).some((command) => command.startsWith('read '))).toBe(
			true,
		);
	} finally {
		await driver.stop();
	}
});

test('an Enter sent with the still-buffered /quit never ends the PTY', async () => {
	const { driver } = await composerStandIn();
	try {
		await driver.send(Buffer.from('/quit'));
		await driver.send(Buffer.from('\r'));
		await expect(driver.waitForEof('codex /quit')).rejects.toThrow('timed out waiting on channel');
	} finally {
		await driver.stop();
	}
});
