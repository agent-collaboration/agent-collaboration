import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { approvalRecordsFrom } from '../../../../verification/observations/providers/acp/codex-approval-outcome.ts';
import type {
	ApprovalOutcomeFile,
	ApprovalRecord,
} from '../../../../verification/observations/providers/acp/codex-approval-outcome.ts';
import { approvalReadback } from './codex-acp-approval-readback.ts';
import type { ApprovalPort, ApprovalReadbackHooks } from './codex-acp-approval-readback.ts';
import { approvalJourneySchema } from './codex-acp-approval-scenes.ts';
import type { ApprovalJourneyDeclaration } from './codex-acp-approval-scenes.ts';

const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

const OPTIONS = [
	{ optionId: 'a', effect: 'allow', scope: 'once', persistentTarget: null },
	{ optionId: 'd', effect: 'decline', scope: 'once', persistentTarget: null },
];
const record = (requestId: string, state: string, options = OPTIONS): ApprovalRecord => {
	const [parsed] = approvalRecordsFrom({ approvals: [{ requestId, state, options }] });
	if (parsed === undefined) throw new Error('fixture record');
	return parsed;
};

interface Setup {
	readonly scratch: string;
	readonly evidence: string;
	readonly declaration: ApprovalJourneyDeclaration;
}
function setup(answerCase: 'allow-once' | 'decline' | 'timeout'): Setup {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'aprb-')));
	roots.push(root);
	const scratch = join(root, 'scratch');
	mkdirSync(scratch, { mode: 0o700 });
	const answer =
		answerCase === 'timeout'
			? { case: 'timeout', actions: [], timeout_ms: 60_000, history_cell: 't', deadline_ms: 90_000 }
			: {
					case: answerCase,
					actions: [{ kind: 'key', key: 'Enter' }],
					cli_decision: answerCase === 'allow-once' ? 'accept' : 'decline',
					history_cell: 'h',
					deadline_ms: 20_000,
				};
	return {
		scratch,
		evidence: root,
		declaration: approvalJourneySchema.parse({
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
			prompt_shown: { prompt_line: 'prompt line', options: [], deadline_ms: 60_000 },
			answer,
			reply_deadline_ms: 120_000,
		}),
	};
}

interface PortScript {
	readonly mode?: string;
	/** Successive answers to `sessionApprovals`; the last one repeats. */
	readonly lists: readonly (readonly ApprovalRecord[])[];
}
function portFor(script: PortScript): ApprovalPort & { reads: number } {
	const port = {
		reads: 0,
		currentMode: (): Promise<string> => Promise.resolve(script.mode ?? 'default'),
		sessionApprovals: (): Promise<readonly ApprovalRecord[]> => {
			const answer = script.lists[Math.min(port.reads, script.lists.length - 1)] ?? [];
			port.reads += 1;
			return Promise.resolve(answer);
		},
	};
	return port;
}
const hooksFor = (input: Setup, port: ApprovalPort, deadlineMs = 3): ApprovalReadbackHooks =>
	approvalReadback({
		port,
		declaration: input.declaration,
		evidence: input.evidence,
		pause: () => Promise.resolve(),
		deadlineMs,
		pollIntervalMs: 1,
	});

async function drive(
	input: Setup,
	lists: PortScript['lists'],
	options: { readonly createFile?: boolean } = {},
): Promise<ApprovalOutcomeFile> {
	const hooks = hooksFor(input, portFor({ lists }));
	await hooks.beforeJourney();
	await hooks.afterPromptShown();
	if (options.createFile === true) writeFileSync(join(input.scratch, 'approval-probe.txt'), 'ok');
	return hooks.afterTurnEnded();
}
const PENDING = [record('r1', 'pendingClientDecision')];

test('allow-once: decided with the file present, and the outcome file is kept', async () => {
	const input = setup('allow-once');
	const outcome = await drive(input, [[], PENDING, [record('r1', 'decided')]], {
		createFile: true,
	});
	expect(outcome).toMatchObject({
		case: 'allow-once',
		final_state: 'decided',
		file_present: true,
		agent_outcome: 'accept',
	});
	const kept = JSON.parse(
		readFileSync(join(input.evidence, 'approval-outcome.json'), 'utf8'),
	) as unknown;
	expect(kept).toEqual(outcome);
});

test('decline: decided with the file absent; timeout: timedOut with the file absent', async () => {
	const declined = await drive(setup('decline'), [[], PENDING, [record('r1', 'decided')]]);
	expect(declined.agent_outcome).toBe('decline');
	const timedOut = await drive(setup('timeout'), [
		[],
		PENDING,
		PENDING,
		[record('r1', 'timedOut')],
	]);
	expect(timedOut).toMatchObject({ final_state: 'timedOut', agent_outcome: 'timedOut' });
});

test('negative: the CLI showed the prompt but the record stays pending', async () => {
	const input = setup('allow-once');
	await expect(drive(input, [[], PENDING], { createFile: true })).rejects.toThrow(
		'the record stays pending after the answer',
	);
	// The evidence is written before the refusal.
	expect(readFileSync(join(input.evidence, 'approval-outcome.json'), 'utf8')).toContain(
		'pendingClientDecision',
	);
});

test('negative: allow answered but the file is absent; decline answered but the file exists', async () => {
	await expect(
		drive(setup('allow-once'), [[], PENDING, [record('r1', 'decided')]]),
	).rejects.toThrow('the scratch file is absent, but the case requires it present');
	await expect(
		drive(setup('decline'), [[], PENDING, [record('r1', 'decided')]], { createFile: true }),
	).rejects.toThrow('the scratch file exists, but the case requires it absent');
});

test('negative: a timeout reported as decided', async () => {
	await expect(drive(setup('timeout'), [[], PENDING, [record('r1', 'decided')]])).rejects.toThrow(
		'record state decided, expected timedOut',
	);
});

test('before any input: a non-default mode, a used scratch directory or a pending approval stops the run', async () => {
	const auto = setup('allow-once');
	await expect(
		hooksFor(auto, portFor({ mode: 'bypassPermissions', lists: [[]] })).beforeJourney(),
	).rejects.toThrow('mode bypassPermissions, not default');
	const used = setup('allow-once');
	writeFileSync(join(used.scratch, 'approval-probe.txt'), 'x');
	await expect(hooksFor(used, portFor({ lists: [[]] })).beforeJourney()).rejects.toThrow(
		'not empty',
	);
	await expect(
		hooksFor(setup('allow-once'), portFor({ lists: [PENDING] })).beforeJourney(),
	).rejects.toThrow('already pending');
});

test('while the prompt shows: no new record, or two, is an error within the poll bound', async () => {
	const none = hooksFor(setup('allow-once'), portFor({ lists: [[]] }));
	await none.beforeJourney();
	await expect(none.afterPromptShown()).rejects.toThrow(
		'no new pending approval record appeared within the 3 ms read deadline (3 polls)',
	);
	const two = hooksFor(
		setup('allow-once'),
		portFor({
			lists: [[], [record('r1', 'pendingClientDecision'), record('r2', 'pendingClientDecision')]],
		}),
	);
	await two.beforeJourney();
	await expect(two.afterPromptShown()).rejects.toThrow('more than one new pending approval');
});

test('the hooks run in order: the turn cannot end before a prompt was read', async () => {
	const input = setup('allow-once');
	const hooks = hooksFor(input, portFor({ lists: [[]] }));
	await expect(hooks.afterTurnEnded()).rejects.toThrow('no pending approval was read');
	await expect(hooks.afterPromptShown()).rejects.toThrow('baseline is absent');
});

test('the read deadline is stated and bounds every wait: a record that never leaves pending fails at it', async () => {
	const input = setup('allow-once');
	let pauses = 0;
	const hooks = approvalReadback({
		port: portFor({ lists: [[], PENDING] }),
		declaration: input.declaration,
		evidence: input.evidence,
		pause: () => {
			pauses += 1;
			return Promise.resolve();
		},
		deadlineMs: 5_000,
		pollIntervalMs: 1_000,
	});
	await hooks.beforeJourney();
	await hooks.afterPromptShown();
	await expect(hooks.afterTurnEnded()).rejects.toThrow('the record stays pending after the answer');
	// Five polls at most: four pauses between them.
	expect(pauses).toBe(4);
	expect(() =>
		approvalReadback({
			port: portFor({ lists: [[]] }),
			declaration: input.declaration,
			evidence: input.evidence,
			pause: () => Promise.resolve(),
			deadlineMs: 500,
			pollIntervalMs: 1_000,
		}),
	).toThrow('interval within deadline');
});
