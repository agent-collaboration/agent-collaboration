import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

import { expect, test } from 'vitest';

import {
	hostedRow,
	inventoryPage,
	inventoryStandIn,
} from '../../../../verification/observations/claude-route-fixture.ts';
import {
	captureClaudeRouteBaseline,
	readClaudeRouteWindow,
} from '../../../../verification/observations/claude-route-window.ts';
import type {
	ClaudeRouteBaseline,
	ClaudeRouteBinding,
} from '../../../../verification/observations/claude-route-window.ts';
import { sourceBinding } from '../evidence/source-binding.ts';
import {
	controlledExecutable,
	initializedSourceRepository,
} from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import { informationJourney } from './codex-acp-information-scenes.ts';
import { promptJourney } from './codex-acp-prompt-scenes.ts';
import {
	checkCodexAcp,
	codexAcpRunPlanSchema,
	codexChild,
	codexJourneyFor,
	driveCodexJourney,
	driveCodexStep,
	exitCodex,
	routeBaselineScene,
} from './codex-acp-run.ts';
import type { CodexAcpRunPlan } from './codex-acp-run.ts';
import { renameJourney } from './codex-acp-scenes.ts';
import {
	ALIAS,
	EPOCH,
	RAW_SESSION,
	SERVICE,
	claudeRouteHostFixture,
	targetFixture,
} from './codex-acp-test-fixtures.ts';

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
const INFORMATION = { kind: 'information', provider: 'cursor', command: 'mcp' };
const REFRESH = {
	kind: 'refresh',
	provider: 'cursor',
	prompt: 'count slowly from one to forty',
	completion: 'REFRESH_DONE_4K7Q',
	reply_deadline_ms: 120_000,
};
const PROMPT = {
	kind: 'prompt',
	provider: 'cursor',
	prompt: 'Reply with only the product of 4321 and 5678, digits only',
	completion: '24534638',
	reply_deadline_ms: 120_000,
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
	const information = await runPlan(INFORMATION);
	expect(codexJourneyFor(information.plan).steps.map((step) => step.checkpoint.scene)).toEqual([
		'mcp-command-ready',
		'mcp-refused',
	]);
	const prompt = await runPlan(PROMPT);
	expect(codexJourneyFor(prompt.plan).steps.map((step) => step.checkpoint.scene)).toEqual([
		'prompt-composed',
		'prompt-replied',
	]);
	const typedAnswer = await runPlan({ ...PROMPT, completion: '4321 and 5678' });
	expect(() => codexJourneyFor(typedAnswer.plan)).toThrow('an echo could satisfy it');
	const refresh = await runPlan(REFRESH);
	expect(codexJourneyFor(refresh.plan).name).toBe('codex-acp-j3-cursor-refresh');
	// The plan shape admits a completion the prompt already holds; the journey refuses it.
	const echoed = await runPlan({ ...REFRESH, completion: 'count slowly' });
	expect(() => codexJourneyFor(echoed.plan)).toThrow('an echo could satisfy it');
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
		// Only /hooks and /mcp with no arguments; no login, option or free slash text.
		{ ...INFORMATION, command: 'login' },
		{ ...INFORMATION, command: '/mcp' },
		{ ...INFORMATION, command: 'mcp verbose' },
		{ ...INFORMATION, args: 'verbose' },
		// An ordinary prompt only, a distinctive completion, a bounded reply and no extra keys.
		{ ...REFRESH, prompt: '/skills' },
		{ ...REFRESH, prompt: 'use $bridge_probe_claude_01a18013' },
		{ ...REFRESH, completion: 'DONE' },
		{ ...REFRESH, reply_deadline_ms: 3_600_000 },
		{ ...REFRESH, toggle: { kind: 'key', key: 'Enter' } },
		// Plain text only: no slash, shell, popup or image input, no extra keys and a bounded reply.
		{ ...PROMPT, prompt: '/model' },
		{ ...PROMPT, prompt: '!echo 24534638' },
		{ ...PROMPT, prompt: 'see @probe.png' },
		{ ...PROMPT, image_path: '/task/probe.png' },
		{ ...PROMPT, actions: [{ kind: 'key', key: 'Enter' }] },
		{ ...PROMPT, completion: '2453' },
		{ ...PROMPT, reply_deadline_ms: 3_600_000 },
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
// Optional Router side effects at the two moments that decide a route baseline's placement: one
// audit row when the typed text arrives, one when the Enter after the drawn frame submits it,
// which then draws the reply frame instead of ending the PTY.
interface RouteStandInEffects {
	readonly auditFile: string;
	readonly onTyped: string;
	readonly onEnter: string;
	readonly replyRows: readonly string[];
}

function standInFrame(rows: readonly string[]): string {
	return Buffer.from(`\u001b[?2026h\u001b[2J\u001b[H${rows.join('\r\n')}\u001b[?2026l`).toString(
		'hex',
	);
}

function composerStandInSource(
	typed: string,
	rows: readonly string[],
	effects: RouteStandInEffects | null = null,
): string {
	const frame = standInFrame(rows);
	return `
const { appendFileSync } = require('node:fs');
let state = 'idle';
const typed = ${JSON.stringify(typed)};
const frame = ${JSON.stringify(frame)};
const effects = ${JSON.stringify(effects === null ? null : { ...effects, replyFrame: standInFrame(effects.replyRows) })};
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
		if (command === 'write' && bytes === typed) {
			state = 'buffered';
			if (effects) appendFileSync(effects.auditFile, effects.onTyped);
		} else if (command === 'write' && bytes === '\\r') {
			if (state === 'drawn' && effects) { appendFileSync(effects.auditFile, effects.onEnter); state = 'replied'; }
			else state = state === 'drawn' ? 'exiting' : 'newline';
		} else if (command === 'read') {
			if (state === 'buffered') { state = 'drawn'; reply = 'BYTES ' + frame; }
			else if (state === 'replied') { state = 'answered'; reply = 'BYTES ' + effects.replyFrame; }
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
	effects: RouteStandInEffects | null = null,
): Promise<{ driver: PtyGridDriver; log: () => string[] }> {
	const root = temporaryRoot();
	const script = controlledExecutable(
		root,
		'drive-terminal',
		composerStandInSource(typed, rows, effects),
	);
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

test('a client that exits after the prompt Enter never reaches the reply scene', async () => {
	const [composed, replied] = promptJourney({
		provider: 'cursor',
		prompt: PROMPT.prompt,
		completion: PROMPT.completion,
		replyDeadlineMs: 8_000,
	}).steps;
	if (composed === undefined || replied === undefined) throw new Error('prompt scene missing');
	const { driver, log } = await composerStandIn(PROMPT.prompt, [`› ${PROMPT.prompt}`]);
	try {
		await driveCodexStep(driver, composed, 'legacy');
		await expect(driveCodexStep(driver, replied, 'legacy')).rejects.toThrow(
			'grid checkpoint unreachable: prompt-replied',
		);
		// The prompt went out as one write and its Enter only after a drawn frame.
		const commands = log();
		const typed = commands.indexOf(`write ${Buffer.from(PROMPT.prompt).toString('hex')}`);
		const enter = commands.indexOf('write 0d');
		expect(commands.slice(typed + 1, enter).some((command) => command.startsWith('read '))).toBe(
			true,
		);
		expect(commands.filter((command) => command.startsWith('write ')).length).toBe(2);
	} finally {
		await driver.stop();
	}
});

test('a client that exits on a refused /hooks never reaches the refused scene', async () => {
	const [ready, refused] = informationJourney('cursor', 'hooks').steps;
	if (ready === undefined || refused === undefined) throw new Error('hooks scene missing');
	const { driver } = await composerStandIn('/hooks', [
		'› /hooks  view and manage lifecycle hooks',
		'',
		'› /hooks',
	]);
	try {
		await driveCodexStep(driver, ready, 'legacy');
		// Enter dispatches the drawn command and this client closes the PTY: an exit is never the
		// usable-composer outcome.
		await expect(driveCodexStep(driver, refused, 'legacy')).rejects.toThrow(
			'grid checkpoint unreachable: hooks-refused',
		);
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

const CLAUDE_ROUTE = {
	host_run_directory: '/tmp/cxr-run',
	host_pid: 5446,
	max_prefix_bytes: 65_536,
	max_segment_bytes: 4096,
	collaboration_cli: { path: '/tmp/cxr-agent-collaboration', sha256: 'a'.repeat(64) },
};
const CLAUDE_PROMPT = { ...PROMPT, provider: 'claude' };

test('a Claude route proof is admitted only for a bounded Claude prompt or selected-skill plan', async () => {
	const { plan } = await runPlan(CURSOR_SKILL);
	const claudeTarget = { ...plan.target, endpoint_id: 'claude-local' };
	const claudePlan = (journey: unknown, route: unknown = CLAUDE_ROUTE): CodexAcpRunPlan =>
		codexAcpRunPlanSchema.parse({ ...plan, target: claudeTarget, journey, claude_route: route });
	expect(routeBaselineScene(claudePlan(CLAUDE_PROMPT))).toBe('prompt-composed');
	expect(routeBaselineScene(claudePlan({ ...CURSOR_SKILL, provider: 'claude' }))).toBe(
		'skill-input-composed',
	);
	expect(routeBaselineScene(plan)).toBeNull();
	for (const [journey, route] of [
		// Another journey, or another provider.
		[{ ...MODEL, provider: 'claude' }, CLAUDE_ROUTE],
		[{ ...INFORMATION, provider: 'claude', command: 'hooks' }, CLAUDE_ROUTE],
		[{ ...REFRESH, provider: 'claude' }, CLAUDE_ROUTE],
		// Unbounded, relative, extra or missing binding fields.
		[CLAUDE_PROMPT, { ...CLAUDE_ROUTE, max_prefix_bytes: 67_108_865 }],
		[CLAUDE_PROMPT, { ...CLAUDE_ROUTE, max_segment_bytes: 1_048_577 }],
		[CLAUDE_PROMPT, { ...CLAUDE_ROUTE, host_run_directory: 'run' }],
		[CLAUDE_PROMPT, { ...CLAUDE_ROUTE, audit_file: '/tmp/other.jsonl' }],
		[CLAUDE_PROMPT, { host_run_directory: '/tmp/cxr-run' }],
		[CLAUDE_PROMPT, { ...CLAUDE_ROUTE, collaboration_cli: { path: 'agent-collaboration' } }],
		[CLAUDE_PROMPT, { ...CLAUDE_ROUTE, collaboration_cli: undefined }],
	] as const)
		expect(() => claudePlan(journey, route), JSON.stringify(journey)).toThrow();
	// The Cursor target never takes a Claude route proof.
	expect(() =>
		codexAcpRunPlanSchema.parse({ ...plan, journey: PROMPT, claude_route: CLAUDE_ROUTE }),
	).toThrow('admitted only for a Claude prompt or selected-skill plan');
});

// Literal Router audit rows (codex-router-core/src/audit.rs:83-98): Codex's own models traffic, a
// rejected Claude local token (http_affinity.rs:110-125) and a forwarded Claude response
// (http_affinity.rs:128-143).
const MODELS_ROW = `{"event_type":"proxy_decision","request_id":"local_proxy_request","route_kind":"models","transport_kind":"http","local_auth_result":"valid","outcome":"allowed","decision_reason":"forwarded","response_commit_state":"committed","account_hash":"acct_openai_77"}\n`;
const REJECTED_CLAUDE_ROW = `{"event_type":"proxy_decision","request_id":"local_proxy_request","route_kind":"claude_messages","transport_kind":"http","local_auth_result":"wrong","outcome":"rejected","decision_reason":"local_auth_wrong","response_commit_state":"not_committed","error_class":"local_auth"}\n`;
const FORWARDED_CLAUDE_ROW = `{"event_type":"proxy_decision","request_id":"local_proxy_request","route_kind":"claude_messages","transport_kind":"http","local_auth_result":"valid","outcome":"allowed","decision_reason":"forwarded","response_commit_state":"committed","account_hash":"acct_5f3a9c0d"}\n`;
const EMPTY_COMPOSER = `› Ask Codex to do anything${' '.repeat(94)}`;

// The shared controlled collaboration CLI answering the one idle managed Session on this
// fixture's service, beside the Host run directory; never the real CLI or Router.
function withInventoryStandIn(host: ReturnType<typeof claudeRouteHostFixture>): {
	readonly binding: ClaudeRouteBinding;
	readonly reads: () => number;
} {
	const cli = inventoryStandIn(dirname(host.runDirectory));
	cli.reply(inventoryPage([hostedRow(RAW_SESSION, 'idle', SERVICE)], SERVICE));
	const binding = { ...host.binding, serviceId: SERVICE, serviceEpoch: EPOCH };
	return {
		binding: { ...binding, collaborationCli: cli.binding },
		reads: () => cli.calls().length,
	};
}

async function promptWithRouteEffects(afterScene: string): Promise<{
	readonly observed: Promise<string[]>;
	readonly baseline: () => ClaudeRouteBaseline | null;
	readonly binding: ClaudeRouteBinding;
	readonly inventoryReads: () => number;
	readonly stop: () => Promise<void>;
}> {
	const host = claudeRouteHostFixture(MODELS_ROW);
	const { binding, reads } = withInventoryStandIn(host);
	const journey = promptJourney({
		provider: 'claude',
		prompt: PROMPT.prompt,
		completion: PROMPT.completion,
		replyDeadlineMs: 8_000,
	});
	const { driver } = await composerStandIn(PROMPT.prompt, [`› ${PROMPT.prompt}`], {
		auditFile: host.auditFile,
		onTyped: REJECTED_CLAUDE_ROW,
		onEnter: FORWARDED_CLAUDE_ROW,
		replyRows: [`• ${PROMPT.completion}`, '', EMPTY_COMPOSER],
	});
	let baseline: ClaudeRouteBaseline | null = null;
	const observed = driveCodexJourney(driver, journey, 'legacy', async () => undefined, {
		afterScene,
		capture: () => {
			baseline = captureClaudeRouteBaseline(binding, {
				afterScene: 'prompt-composed',
				providerSessionId: RAW_SESSION,
				nativeThreadId: ALIAS,
			});
		},
	});
	return {
		observed,
		baseline: () => baseline,
		binding,
		inventoryReads: reads,
		stop: async (): Promise<void> => {
			await driver.stop();
		},
	};
}

test('the route baseline lands after the composed prompt is drawn and before its Enter', async () => {
	const run = await promptWithRouteEffects('prompt-composed');
	try {
		expect(await run.observed).toEqual(['prompt-composed', 'prompt-replied']);
	} finally {
		await run.stop();
	}
	const baseline = run.baseline();
	if (baseline === null) throw new Error('route baseline never captured');
	// After composition: the typed-time row is in the prefix. Before Enter: the submission's
	// forwarded row is not. A baseline moved earlier would carry the rejected row into the
	// window; one moved after Enter would leave the window without the forwarded row.
	expect(baseline.file).toMatchObject({
		state: 'present',
		byte_length: Buffer.byteLength(MODELS_ROW + REJECTED_CLAUDE_ROW),
	});
	// The pre-Enter inventory read ran once at this hook, before the audit baseline.
	expect(run.inventoryReads()).toBe(1);
	expect(baseline.inventory_before).toMatchObject({ session_id: RAW_SESSION, state: 'idle' });
	const window = readClaudeRouteWindow(baseline, run.binding);
	expect({ rows: window.rows, bytes: window.segment_bytes }).toEqual({
		rows: 1,
		bytes: Buffer.byteLength(FORWARDED_CLAUDE_ROW),
	});
	expect(window.categories).toEqual([
		{
			route_kind: 'claude_messages',
			transport_kind: 'http',
			local_auth_result: 'valid',
			outcome: 'allowed',
			decision_reason: 'forwarded',
			response_commit_state: 'committed',
			error_class: null,
			account_hash_present: true,
			count: 1,
		},
	]);
});

test('the route hook refuses a scene that the submission Enter does not directly follow', async () => {
	const run = await promptWithRouteEffects('prompt-replied');
	try {
		await expect(run.observed).rejects.toThrow('must directly precede the submission Enter');
	} finally {
		await run.stop();
	}
	expect(run.baseline()).toBeNull();
	expect(run.inventoryReads()).toBe(0);
});
