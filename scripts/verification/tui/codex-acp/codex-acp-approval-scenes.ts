/**
 * The O5 approval journey on a Claude Haiku Session (not in auto mode): a trigger prompt asks the
 * agent to create a named file inside the run's own scratch directory; the CLI shows the approval
 * prompt; the step answers allow-once, declines, or waits for the timeout, per the declared case.
 * One case per run. The CLI's prompt and answer texts are declared by the plan (0.166.0 texts are
 * ungrounded until a discovery run); the agent-side outcome is read by
 * codex-acp-approval-readback.ts. Built and unit-tested only; the live run is D4's.
 */
import { lstatSync, readdirSync, realpathSync } from 'node:fs';
import { isAbsolute } from 'node:path';

import { z } from 'zod';

import { codexOrdinaryPromptSchema, requireOrdinaryReply } from './codex-acp-prompt-scenes.ts';
import {
	CODEX_0_165_TEXT,
	UI_CHECKPOINT_MS,
	codexActionSchema,
	codexJourneySchema,
} from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';

const deadlineSchema = z.number().int().positive().max(600_000);
const declaredTextSchema = z
	.string()
	.min(1)
	.refine((text) => !/\p{Cc}/u.test(text), 'control character');

/** One answer case: the CLI actions, the decision they put on the wire, and what the CLI then shows. */
const answerCommon = {
	// The history text the CLI draws once the prompt is resolved this way.
	history_cell: declaredTextSchema,
	// An error cell this case is expected to draw (text after `■ `), or none.
	expected_error: declaredTextSchema.optional(),
	deadline_ms: deadlineSchema,
};

const answeredFields = {
	actions: z.array(codexActionSchema).min(1),
	// The decision string the CLI's answer carries on the wire.
	cli_decision: declaredTextSchema,
	...answerCommon,
};
const allowOnceCaseSchema = z.strictObject({ case: z.literal('allow-once'), ...answeredFields });
const allowSessionCaseSchema = z.strictObject({
	case: z.literal('allow-session'),
	...answeredFields,
});
const declineCaseSchema = z.strictObject({ case: z.literal('decline'), ...answeredFields });

const timeoutCaseSchema = z
	.strictObject({
		case: z.literal('timeout'),
		// The CLI is left alone: no action answers the prompt.
		actions: z.array(codexActionSchema).max(0),
		timeout_ms: deadlineSchema,
		...answerCommon,
	})
	.refine((answer) => answer.deadline_ms > answer.timeout_ms, {
		message: 'the timeout case waits longer than the declared approval timeout',
	});

export const approvalJourneySchema = z
	.strictObject({
		kind: z.literal('approval'),
		// The same trigger runs on real Codex (the reference) and on a Claude Haiku Router Session.
		provider: z.enum(['claude', 'codex']),
		// The Router Session's current mode, read back before any input: never an auto or
		// edit-accepting mode. Real Codex has no such Router Session mode (null).
		session_mode: z.literal('default').nullable(),
		// The kind of action the trigger needs permission for; real Codex and the Router must agree.
		action_kind: z.enum(['command', 'fileChange']),
		// The Session's cwd and the only place the trigger may write: a fresh private directory.
		scratch_directory: z.string().refine(isAbsolute, 'absolute path required'),
		file_name: z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._-]{0,39}$/u),
		trigger: z.strictObject({
			// An ordinary prompt that names the file and asks for a reply line it never types.
			prompt: codexOrdinaryPromptSchema,
			completion: z.string().min(8),
		}),
		prompt_shown: z.strictObject({
			// A line only the approval prompt draws, plus any option rows it must list.
			prompt_line: declaredTextSchema,
			options: z.array(declaredTextSchema),
			deadline_ms: deadlineSchema,
		}),
		answer: z.discriminatedUnion('case', [
			allowOnceCaseSchema,
			allowSessionCaseSchema,
			declineCaseSchema,
			timeoutCaseSchema,
		]),
		// The second window, attached while the approving window answers: it must draw the result.
		window_b: z.strictObject({ result_text: declaredTextSchema }),
		reply_deadline_ms: deadlineSchema,
	})
	.refine((journey) => (journey.provider === 'claude') === (journey.session_mode !== null), {
		message:
			'a Claude approval run declares its Session mode; the real-Codex reference declares none',
	})
	.refine((journey) => journey.provider !== 'codex' || journey.answer.case !== 'timeout', {
		message: 'real Codex has no approval timeout to declare',
	})
	.refine((journey) => journey.trigger.prompt.includes(journey.file_name), {
		message: 'the trigger prompt names the scratch file',
	})
	.refine((journey) => !journey.trigger.completion.includes(journey.file_name), {
		message: 'the completion line contains the file name; the prompt echo could satisfy it',
	})
	.refine((journey) => !journey.prompt_shown.prompt_line.includes(journey.trigger.prompt), {
		message: 'the approval prompt line repeats the typed prompt',
	});
export type ApprovalJourneyDeclaration = z.infer<typeof approvalJourneySchema>;

/** The scratch directory must lie strictly inside the run's output directory. */
export function scratchInsideOutput(scratchDirectory: string, output: string): boolean {
	return scratchDirectory.startsWith(`${output}/`) && !scratchDirectory.includes('/../');
}

/** The second window: attached before the answer, it must draw the declared result. */
export function approvalWatcherJourney(input: ApprovalJourneyDeclaration): CodexJourney {
	const journey = approvalJourneySchema.parse(input);
	return codexJourneySchema.parse({
		name: `codex-acp-o5-${journey.provider}-approval-${journey.answer.case}-b`,
		steps: [
			{
				actions: [],
				checkpoint: {
					scene: 'window-b-sees-result',
					require: [journey.window_b.result_text],
					forbid: [journey.prompt_shown.prompt_line, CODEX_0_165_TEXT.errorPrefix],
					deadline_ms: journey.reply_deadline_ms,
				},
			},
		],
		whole_run_errors: 'forbid',
	});
}

/**
 * Trigger, prompt shown, answer, and the turn's end. The whole-run error rule is scoped to the
 * answer's declared error cell, or forbids every error when the case declares none.
 */
export function approvalJourney(input: ApprovalJourneyDeclaration): CodexJourney {
	const journey = approvalJourneySchema.parse(input);
	const { trigger, prompt_shown: shown, answer } = journey;
	requireOrdinaryReply(trigger.prompt, trigger.completion, []);
	const errors = answer.expected_error === undefined ? [CODEX_0_165_TEXT.errorPrefix] : [];
	return codexJourneySchema.parse({
		name: `codex-acp-o5-${journey.provider}-approval-${answer.case}`,
		steps: [
			{
				actions: [{ kind: 'text', text: trigger.prompt }],
				checkpoint: {
					scene: 'approval-trigger-composed',
					require: [CODEX_0_165_TEXT.composerPrompt + trigger.prompt],
					forbid: [
						trigger.completion,
						shown.prompt_line,
						CODEX_0_165_TEXT.working,
						CODEX_0_165_TEXT.resumingSession,
						CODEX_0_165_TEXT.errorPrefix,
					],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: 'approval-prompt-shown',
					require: [shown.prompt_line, ...shown.options],
					forbid: [trigger.completion, answer.history_cell, CODEX_0_165_TEXT.errorPrefix],
					deadline_ms: shown.deadline_ms,
				},
			},
			{
				actions: [...answer.actions],
				checkpoint: {
					scene: 'approval-answered',
					require: [answer.history_cell],
					forbid: [shown.prompt_line, trigger.completion, ...errors],
					deadline_ms: answer.deadline_ms,
					...(answer.expected_error === undefined ? {} : { error_line: answer.expected_error }),
				},
			},
			{
				actions: [],
				checkpoint: {
					scene: 'approval-turn-ended',
					require: [trigger.completion],
					forbid: [shown.prompt_line, CODEX_0_165_TEXT.working, ...errors],
					deadline_ms: journey.reply_deadline_ms,
					empty_composer: true,
				},
			},
		],
		whole_run_errors: answer.expected_error === undefined ? 'forbid' : 'scoped',
	});
}

/**
 * The scratch directory before the run: a real directory (not a symlink), owned by this user,
 * private (no group or other access), and empty. A reused or shared directory could already hold
 * the file, so a "present" reading would prove nothing.
 */
export function requireFreshScratch(directory: string): void {
	const status = lstatSync(directory);
	if (!status.isDirectory() || realpathSync(directory) !== directory)
		throw new Error('the scratch directory is not a real directory at its declared path');
	if (status.uid !== process.getuid?.())
		throw new Error('the scratch directory is not owned by this user');
	if ((status.mode & 0o077) !== 0) throw new Error('the scratch directory is not private');
	if (readdirSync(directory).length > 0) throw new Error('the scratch directory is not empty');
}

export type ScratchFileState = 'present' | 'absent';

/**
 * Whether the declared file exists in the scratch directory after the turn. Anything else in the
 * directory, or a file that is not a regular file, is an error: the reading must be about the
 * declared file alone.
 */
export function readScratchFile(directory: string, fileName: string): ScratchFileState {
	const entries = readdirSync(directory);
	const strangers = entries.filter((entry) => entry !== fileName);
	if (strangers.length > 0)
		throw new Error(`the scratch directory holds other entries: ${strangers.join(', ')}`);
	if (!entries.includes(fileName)) return 'absent';
	if (!lstatSync(`${directory}/${fileName}`).isFile())
		throw new Error('the scratch file is not a regular file');
	return 'present';
}
