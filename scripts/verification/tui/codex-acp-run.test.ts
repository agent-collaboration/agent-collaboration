import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import {
	checkCodexAcp,
	codexAcpRunPlanSchema,
	codexChild,
	codexJourneyFor,
	driveCodexStep,
	exitCodex,
} from './codex-acp-run.ts';
import type { CodexAcpRunPlan } from './codex-acp-run.ts';
import { renameJourney } from './codex-acp-scenes.ts';
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

const PICKER = { kind: 'picker', provider: 'claude', skill: 'bridge_probe_claude_01a18013' };
const REOPEN = {
	kind: 'reopen',
	provider: 'cursor',
	prior_reply: ['ACP_BRIDGE_CURSOR_A_9C31E4|ALPHA|RED'],
};
const RENAME = {
	kind: 'rename',
	provider: 'cursor',
	title: 'Fixture Title',
	expected_refusal: ['Renaming this thread is not supported here'],
};
const MODEL = {
	kind: 'model',
	provider: 'cursor',
	catalogue_page: ['Grok 4.6', 'Composer 2.5'],
	stale_rows: ['Use provider settings'],
	model_navigation: [{ kind: 'key', key: 'Down' }],
	target: { display_name: 'Composer 2.5', number: 2, is_current: false },
	effort: null,
	apply: 'session',
	expected_outcome: ['Model changed to fixture-model', 'for this session only'],
	outcome_deadline_ms: 30_000,
};

test('each declared journey kind builds its scenes for the target provider only', async () => {
	const { plan } = await runPlan(CURSOR_SKILL);
	expect(codexJourneyFor(plan).name).toBe('codex-acp-j1-cursor-skill');
	const compact = await runPlan({
		kind: 'compact',
		provider: 'cursor',
		effect_deadline_ms: 120_000,
	});
	expect(codexJourneyFor(compact.plan).steps.map((step) => step.checkpoint.scene)).toEqual([
		'compact-command-ready',
		'compact-refused',
	]);
	const interrupt = await runPlan({
		kind: 'interrupt',
		provider: 'cursor',
		prompt: 'count to fifty',
	});
	expect(codexJourneyFor(interrupt.plan).name).toBe('codex-acp-j3-cursor-interrupt');
	const picker = await runPlan({ ...PICKER, provider: 'cursor' });
	expect(codexJourneyFor(picker.plan).name).toBe('codex-acp-j1-cursor-picker-only');
	const reopen = await runPlan(REOPEN);
	expect(codexJourneyFor(reopen.plan).steps[0]?.checkpoint.require).toEqual(REOPEN.prior_reply);
	const rename = await runPlan(RENAME);
	expect(codexJourneyFor(rename.plan).steps.map((step) => step.checkpoint.scene)).toEqual([
		'rename-command-ready',
		'rename-refused',
	]);
	const model = await runPlan(MODEL);
	const modelJourney = codexJourneyFor(model.plan);
	expect(modelJourney.steps.map((step) => step.checkpoint.scene)).toEqual([
		'model-command-ready',
		'model-catalogue-ready',
		'model-row-selected',
		'model-applied',
	]);
	expect(modelJourney.steps[2]?.actions).toEqual(MODEL.model_navigation);
	expect(modelJourney.steps[3]?.actions).toEqual([{ kind: 'text', text: 's' }]);
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
	// A reopen with nothing to find, a model step with no keys or outcome, and an unbounded
	// model outcome wait are refused like the other journeys.
	for (const journey of [
		{ ...REOPEN, prior_reply: [] },
		{ ...MODEL, apply: 'global' },
		// Hard cutover: the old free navigation field and non-menu keys are refused.
		{ ...MODEL, navigation: [{ kind: 'key', key: 'Down' }] },
		{ ...MODEL, model_navigation: [{ kind: 'key', key: 'Enter' }] },
		{ ...MODEL, model_navigation: [{ kind: 'text', text: 's' }] },
		{ ...MODEL, catalogue_page: [] },
		{ ...MODEL, catalogue_page: Array.from({ length: 9 }, (_, index) => `Model ${index}`) },
		{ ...MODEL, target: { ...MODEL.target, number: 0 } },
		{ ...MODEL, expected_outcome: [] },
		{ ...MODEL, outcome_deadline_ms: 3_600_000 },
		{ ...PICKER, skill: '' },
		{ ...RENAME, title: 'Fixture\r/quit' },
		{ ...RENAME, expected_refusal: [] },
		{ ...RENAME, command: '/rename x' },
	])
		expect(() => codexAcpRunPlanSchema.parse({ ...plan, journey })).toThrow();
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
// the PTY never ends. Enter after the drawn rows dispatches the typed command and the PTY ends,
// as /quit does, and as today's client does when a refused /rename takes the CodexOp fatal path.
function composerStandInSource(typed: string, rows: readonly string[]): string {
	const frame = Buffer.from(
		`\u001b[?2026h\u001b[2J\u001b[H${rows.join('\r\n')}\u001b[?2026l`,
	).toString('hex');
	return `
const { appendFileSync } = require('node:fs');
let state = 'idle';
const typed = ${JSON.stringify(typed)};
const frame = ${JSON.stringify(frame)};
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
		if (command === 'write' && bytes === typed) state = 'buffered';
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
}
const QUIT_ROWS = ['› /quit', '  /quit     exit Codex'] as const;

async function composerStandIn(
	typed = '/quit',
	rows: readonly string[] = QUIT_ROWS,
): Promise<{ driver: PtyGridDriver; log: () => string[] }> {
	const root = temporaryRoot();
	const script = controlledExecutable(root, 'drive-terminal', composerStandInSource(typed, rows));
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

test('a client that exits on the refused /rename never reaches the recovery scene', async () => {
	const journey = renameJourney('cursor', 'Fixture Title', [
		'Renaming this thread is not supported here',
	]);
	const [ready, refused] = journey.steps;
	if (ready === undefined || refused === undefined) throw new Error('rename scene missing');
	const { driver, log } = await composerStandIn('/rename Fixture Title', [
		'› /rename Fixture Title',
	]);
	try {
		await driveCodexStep(driver, ready, 'legacy');
		// The Enter dispatches the drawn command; this client then closes the PTY, as the CodexOp
		// fatal path does today. An exit is never the recovery outcome.
		await expect(driveCodexStep(driver, refused, 'legacy')).rejects.toThrow(
			'grid checkpoint unreachable: rename-refused',
		);
		const commands = log();
		const typed = commands.indexOf(`write ${Buffer.from('/rename Fixture Title').toString('hex')}`);
		const enter = commands.indexOf('write 0d');
		expect(commands.slice(typed + 1, enter).some((command) => command.startsWith('read '))).toBe(
			true,
		);
	} finally {
		await driver.stop();
	}
});
