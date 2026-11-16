import {
	chmodSync,
	mkdirSync,
	mkdtempSync,
	realpathSync,
	rmSync,
	symlinkSync,
	writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import {
	approvalJourney,
	approvalJourneySchema,
	approvalWatcherJourney,
	readScratchFile,
	requireFreshScratch,
	scratchInsideOutput,
} from './codex-acp-approval-scenes.ts';
import type { ApprovalJourneyDeclaration } from './codex-acp-approval-scenes.ts';
import { expectedErrorCells, gridShowsCheckpoint } from './codex-acp-scenes.ts';

const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});
function privateRoot(): string {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'apr-')));
	roots.push(root);
	return root;
}

const EMPTY_COMPOSER = `› Ask Codex to do anything${' '.repeat(94)}`;
const BASE = {
	kind: 'approval',
	provider: 'claude',
	session_mode: 'default',
	action_kind: 'command',
	window_b: { result_text: 'DISCOVERY-b-result' },
	scratch_directory: '/tmp/run/scratch',
	file_name: 'approval-probe.txt',
	trigger: {
		prompt: 'Create approval-probe.txt here, then reply with only the product of 4321 and 5678',
		completion: 'ANSWER 24534638',
	},
	prompt_shown: {
		prompt_line: 'DISCOVERY-approval-prompt-line',
		options: ['DISCOVERY-option-allow-once', 'DISCOVERY-option-decline'],
		deadline_ms: 60_000,
	},
	reply_deadline_ms: 120_000,
} as const;
const ALLOW = {
	case: 'allow-once',
	actions: [{ kind: 'key', key: 'Enter' }],
	cli_decision: 'DISCOVERY-accept',
	history_cell: 'DISCOVERY-you-approved',
	deadline_ms: 20_000,
} as const;
const DECLINE = {
	case: 'decline',
	actions: [
		{ kind: 'key', key: 'Down' },
		{ kind: 'key', key: 'Enter' },
	],
	cli_decision: 'DISCOVERY-decline',
	history_cell: 'DISCOVERY-you-declined',
	deadline_ms: 20_000,
} as const;
const TIMEOUT = {
	case: 'timeout',
	actions: [],
	timeout_ms: 60_000,
	history_cell: 'DISCOVERY-timed-out',
	deadline_ms: 90_000,
} as const;
const declare = (answer: unknown): ApprovalJourneyDeclaration =>
	approvalJourneySchema.parse({ ...BASE, answer });

test("each case is trigger, prompt shown, the case's answer, then the turn ends", () => {
	for (const answer of [ALLOW, DECLINE, TIMEOUT]) {
		const journey = approvalJourney(declare(answer));
		expect(journey.name).toBe(`codex-acp-o5-claude-approval-${answer.case}`);
		expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
			'approval-trigger-composed',
			'approval-prompt-shown',
			'approval-answered',
			'approval-turn-ended',
		]);
		expect(journey.whole_run_errors).toBe('forbid');
	}
	// The timeout answers nothing and waits past its declared timeout.
	const timeout = approvalJourney(declare(TIMEOUT));
	expect(timeout.steps[2]?.actions).toEqual([]);
	expect(timeout.steps[2]?.checkpoint.deadline_ms).toBeGreaterThan(TIMEOUT.timeout_ms);
});

test('an expected error cell scopes the whole-run rule to exactly that cell', () => {
	const journey = approvalJourney(
		declare({ ...DECLINE, expected_error: 'DISCOVERY-declined-error' }),
	);
	expect(journey.whole_run_errors).toBe('scoped');
	expect(expectedErrorCells(journey)).toEqual(['DISCOVERY-declined-error']);
});

test('negative: the CLI shows the prompt but the answer never takes effect', () => {
	const journey = approvalJourney(declare(ALLOW));
	const answered = journey.steps[2]?.checkpoint;
	if (answered === undefined) throw new Error('no answered step');
	expect(gridShowsCheckpoint(['DISCOVERY-you-approved'], answered)).toBe(true);
	// The prompt is still drawn: the answer was not taken.
	expect(
		gridShowsCheckpoint(['DISCOVERY-approval-prompt-line', 'DISCOVERY-you-approved'], answered),
	).toBe(false);
	const ended = journey.steps[3]?.checkpoint;
	if (ended === undefined) throw new Error('no ended step');
	expect(gridShowsCheckpoint(['ANSWER 24534638', EMPTY_COMPOSER], ended)).toBe(true);
	expect(gridShowsCheckpoint(['ANSWER 24534638', '• Working'], ended)).toBe(false);
});

test('negative: a prompt that never appears, and a reply that arrives before the prompt', () => {
	const shown = approvalJourney(declare(ALLOW)).steps[1]?.checkpoint;
	if (shown === undefined) throw new Error('no shown step');
	const lines = [
		'DISCOVERY-approval-prompt-line',
		'DISCOVERY-option-allow-once',
		'DISCOVERY-option-decline',
	];
	expect(gridShowsCheckpoint(lines, shown)).toBe(true);
	expect(gridShowsCheckpoint(lines.slice(0, 2), shown)).toBe(false);
	expect(gridShowsCheckpoint([...lines, 'ANSWER 24534638'], shown)).toBe(false);
});

test('schema refuses an approval off the Claude provider, a mode that auto-approves and a bad pairing', () => {
	const refuse = (patch: Record<string, unknown>): void => {
		expect(() => approvalJourneySchema.parse({ ...BASE, answer: ALLOW, ...patch })).toThrow();
	};
	refuse({ provider: 'codex' });
	refuse({ provider: 'cursor' });
	refuse({ session_mode: 'bypassPermissions' });
	refuse({ session_mode: 'acceptEdits' });
	refuse({
		trigger: {
			...BASE.trigger,
			prompt: 'Make a file, then reply with the product of 4321 and 5678',
		},
	});
	refuse({ file_name: '../escape.txt' });
	refuse({ trigger: { ...BASE.trigger, completion: 'file approval-probe.txt' } });
	refuse({ scratch_directory: 'relative/dir' });
	refuse({ answer: { ...TIMEOUT, deadline_ms: 30_000 } });
	refuse({ answer: { ...TIMEOUT, actions: [{ kind: 'key', key: 'Enter' }] } });
	refuse({ answer: { ...ALLOW, actions: [] } });
});

test('the scratch directory must lie strictly inside the run output', () => {
	expect(scratchInsideOutput('/out/run/scratch', '/out/run')).toBe(true);
	expect(scratchInsideOutput('/out/run', '/out/run')).toBe(false);
	expect(scratchInsideOutput('/elsewhere/scratch', '/out/run')).toBe(false);
	expect(scratchInsideOutput('/out/run/../x/scratch', '/out/run')).toBe(false);
	expect(scratchInsideOutput('/out/run-2/scratch', '/out/run')).toBe(false);
});

test('a fresh private empty directory passes; shared, reused or linked ones fail', () => {
	const root = privateRoot();
	const fresh = join(root, 'fresh');
	mkdirSync(fresh, { mode: 0o700 });
	expect(() => requireFreshScratch(fresh)).not.toThrow();
	const open = join(root, 'open');
	mkdirSync(open);
	chmodSync(open, 0o755);
	expect(() => requireFreshScratch(open)).toThrow('not private');
	const used = join(root, 'used');
	mkdirSync(used, { mode: 0o700 });
	writeFileSync(join(used, 'approval-probe.txt'), 'x');
	expect(() => requireFreshScratch(used)).toThrow('not empty');
	const link = join(root, 'link');
	symlinkSync(fresh, link);
	expect(() => requireFreshScratch(link)).toThrow('not a real directory');
	expect(() => requireFreshScratch(join(root, 'missing'))).toThrow();
});

test('the scratch file reading is about the declared file alone', () => {
	const root = privateRoot();
	const scratch = join(root, 'scratch');
	mkdirSync(scratch, { mode: 0o700 });
	expect(readScratchFile(scratch, 'approval-probe.txt')).toBe('absent');
	writeFileSync(join(scratch, 'approval-probe.txt'), 'ok');
	expect(readScratchFile(scratch, 'approval-probe.txt')).toBe('present');
	writeFileSync(join(scratch, 'stray.txt'), 'x');
	expect(() => readScratchFile(scratch, 'approval-probe.txt')).toThrow('other entries: stray.txt');
	const odd = join(root, 'odd');
	mkdirSync(odd, { mode: 0o700 });
	mkdirSync(join(odd, 'approval-probe.txt'));
	expect(() => readScratchFile(odd, 'approval-probe.txt')).toThrow('not a regular file');
});

const SESSION = {
	...ALLOW,
	case: 'allow-session',
	cli_decision: 'DISCOVERY-accept-session',
} as const;

test('allow-for-the-session is a third answered case with its own wire decision', () => {
	const journey = approvalJourney(declare(SESSION));
	expect(journey.name).toBe('codex-acp-o5-claude-approval-allow-session');
});

test('the real-Codex reference declares no Session mode and no timeout; a Claude run must declare its mode', () => {
	const reference = { ...BASE, provider: 'codex', session_mode: null, answer: ALLOW };
	expect(approvalJourneySchema.parse(reference).provider).toBe('codex');
	expect(() => approvalJourneySchema.parse({ ...reference, answer: TIMEOUT })).toThrow(
		'real Codex has no approval timeout',
	);
	expect(() => approvalJourneySchema.parse({ ...reference, session_mode: 'default' })).toThrow(
		'declares its Session mode',
	);
	expect(() => approvalJourneySchema.parse({ ...BASE, session_mode: null, answer: ALLOW })).toThrow(
		'declares its Session mode',
	);
	expect(() =>
		approvalJourneySchema.parse({ ...BASE, action_kind: 'network', answer: ALLOW }),
	).toThrow();
});

test('the second window must draw the declared result and not the approval prompt', () => {
	const watcher = approvalWatcherJourney(declare(ALLOW));
	const seen = watcher.steps[0]?.checkpoint;
	if (seen === undefined) throw new Error('no watcher step');
	expect(watcher.steps[0]?.actions).toEqual([]);
	expect(gridShowsCheckpoint(['DISCOVERY-b-result'], seen)).toBe(true);
	// The result never reached B, or B was shown the prompt itself.
	expect(gridShowsCheckpoint([EMPTY_COMPOSER], seen)).toBe(false);
	expect(gridShowsCheckpoint(['DISCOVERY-b-result', 'DISCOVERY-approval-prompt-line'], seen)).toBe(
		false,
	);
});
