/**
 * Codex ACP journey scenes (J1 skills, J2 commands, J3 preservation) in the unchanged Codex 0.165
 * TUI: the keys a user presses and the grid text each step must reach before the next one.
 * Expected text comes from Codex source at 360319d8 (anchors in CODEX_0_165_TEXT) and from the
 * declared journey input, never from an earlier capture. Every scene stays unobserved until a
 * declared run drives it on the shared wterm grid.
 */
import { z } from 'zod';

import { encodeJourneyKey } from './journey-key.ts';
import type { KeyProtocol } from './pty-grid-driver.ts';

export const codexKeySchema = z.enum(['Enter', 'Escape', 'Tab', 'Up', 'Down', 'Ctrl+C']);
export const codexActionSchema = z.discriminatedUnion('kind', [
	z.strictObject({ kind: z.literal('text'), text: z.string().min(1) }),
	z.strictObject({ kind: z.literal('key'), key: codexKeySchema }),
	// Bracketed paste: a pasted absolute image path becomes an attachment
	// (paste_input.rs:193-199, chat_composer.rs:1254-1267).
	z.strictObject({ kind: z.literal('paste'), text: z.string().min(1) }),
]);
export type CodexAction = z.infer<typeof codexActionSchema>;

export const codexCheckpointSchema = z.strictObject({
	scene: z.string().regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/u),
	// Literal text that must all appear on the grid, and text that must not appear.
	require: z.array(z.string().min(1)).min(1),
	forbid: z.array(z.string().min(1)),
	// The declared bound for this checkpoint; the wait itself ends on a matching grid frame.
	deadline_ms: z.number().int().positive().max(600_000),
});
export type CodexCheckpoint = z.infer<typeof codexCheckpointSchema>;

export const codexJourneySchema = z
	.strictObject({
		name: z.string().regex(/^codex-acp-[a-z0-9]+(?:-[a-z0-9]+)*$/u),
		steps: z
			.array(
				z.strictObject({
					actions: z.array(codexActionSchema),
					checkpoint: codexCheckpointSchema,
				}),
			)
			.min(1),
	})
	.refine(
		(journey) =>
			new Set(journey.steps.map((step) => step.checkpoint.scene)).size === journey.steps.length,
		'duplicate checkpoint scene',
	);
export type CodexJourney = z.infer<typeof codexJourneySchema>;

/** Text the unchanged Codex 0.165 TUI renders, with its source anchor at 360319d8. */
export const CODEX_0_165_TEXT = {
	// bottom_pane/skill_popup.rs:267-273 with key_hint.rs:109 (Enter) and lowercase Esc.
	skillPopupHint: 'enter insert · esc close',
	// bottom_pane/chat_composer.rs:228-242: first local image placeholder.
	localImage: '[Image #1]',
	// chatwidget/compaction.rs:43, completed explicit compaction.
	contextCompacted: 'Context compacted',
	// history_cell/notices.rs:331-336, every error history cell.
	errorPrefix: '■ ',
	// chatwidget/input_restore.rs:383, interrupted turn.
	interrupted: '■ Conversation interrupted',
	// chatwidget/model_popups.rs:279.
	modelPopupTitle: 'Select Model and Effort',
	// bottom_pane/mod.rs:1306, default status header while a turn runs.
	working: 'Working',
} as const;

/** UI and metadata checkpoints keep the shared driver's existing bound. */
export const UI_CHECKPOINT_MS = 8_000;

export interface SkillJourneyInput {
	readonly provider: 'claude' | 'cursor';
	// Selected through the picker in this order. Claude prepares exactly one selected skill (C2);
	// Cursor accepts one or more distinct selections.
	readonly skills: readonly string[];
	// Text before the first selected skill; empty for a leading selection.
	readonly prefix: string;
	readonly args: string;
	// Provider-owned reply text each selected skill's own instructions produce, never typed.
	readonly expectedEffects: readonly string[];
	readonly imagePath: string | null;
	readonly effectDeadlineMs: number;
}

/** The exact text Codex submits: each picker insertion adds `$skill` and a separator space. */
export function composedSkillText(input: SkillJourneyInput): string {
	return `${input.prefix}${input.skills.map((skill) => `$${skill}`).join(' ')} ${input.args}`;
}

function requireSkillJourneyInput(input: SkillJourneyInput): void {
	if (input.skills.length === 0 || new Set(input.skills).size !== input.skills.length)
		throw new Error('skill journey needs distinct selected skills');
	if (input.provider === 'claude' && input.skills.length !== 1)
		throw new Error('Claude prepares exactly one selected skill');
	const typed = [composedSkillText(input), ...input.skills];
	if (
		input.expectedEffects.length === 0 ||
		input.expectedEffects.some((effect) => typed.some((text) => text.includes(effect)))
	)
		throw new Error('an expected effect appears in the typed input; an echo could satisfy it');
}

/** J1: select each skill through the `$` picker, compose arguments (and image), submit. */
export function skillJourney(input: SkillJourneyInput): CodexJourney {
	requireSkillJourneyInput(input);
	const selections = input.skills.flatMap((skill, index) => [
		{
			actions: [{ kind: 'text' as const, text: `${index === 0 ? input.prefix : ''}$${skill}` }],
			checkpoint: {
				scene: `skill-picker-open-${index + 1}`,
				require: [skill, CODEX_0_165_TEXT.skillPopupHint],
				forbid: [CODEX_0_165_TEXT.errorPrefix],
				deadline_ms: UI_CHECKPOINT_MS,
			},
		},
		{
			actions: [{ kind: 'key' as const, key: 'Tab' as const }],
			checkpoint: {
				scene: `skill-selected-${index + 1}`,
				require: [`$${skill}`],
				forbid: [CODEX_0_165_TEXT.skillPopupHint],
				deadline_ms: UI_CHECKPOINT_MS,
			},
		},
	]);
	return codexJourneySchema.parse({
		name: `codex-acp-j1-${input.provider}-skill`,
		steps: [
			...selections,
			{
				actions: [
					{ kind: 'text', text: input.args },
					...(input.imagePath === null ? [] : [{ kind: 'paste' as const, text: input.imagePath }]),
				],
				checkpoint: {
					scene: 'skill-input-composed',
					require: [
						composedSkillText(input),
						...(input.imagePath === null ? [] : [CODEX_0_165_TEXT.localImage]),
					],
					forbid: [CODEX_0_165_TEXT.skillPopupHint],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: 'skill-effect',
					require: [...input.expectedEffects],
					forbid: [CODEX_0_165_TEXT.errorPrefix],
					deadline_ms: input.effectDeadlineMs,
				},
			},
		],
	});
}

/** J2: `/compact` completes for Claude; Cursor shows an error and keeps input usable. */
export function compactJourney(
	provider: 'claude' | 'cursor',
	effectDeadlineMs: number,
): CodexJourney {
	const supported = provider === 'claude';
	return codexJourneySchema.parse({
		name: `codex-acp-j2-${provider}-compact`,
		steps: [
			{
				actions: [
					{ kind: 'text', text: '/compact' },
					{ kind: 'key', key: 'Enter' },
				],
				checkpoint: supported
					? {
							scene: 'compact-completed',
							require: [CODEX_0_165_TEXT.contextCompacted],
							forbid: [CODEX_0_165_TEXT.errorPrefix],
							deadline_ms: effectDeadlineMs,
						}
					: {
							scene: 'compact-refused',
							require: [CODEX_0_165_TEXT.errorPrefix],
							forbid: [CODEX_0_165_TEXT.contextCompacted],
							deadline_ms: UI_CHECKPOINT_MS,
						},
			},
		],
	});
}

/** J3: a running turn is interrupted from the keyboard and the conversation stays usable. */
export function interruptJourney(provider: 'claude' | 'cursor', longPrompt: string): CodexJourney {
	return codexJourneySchema.parse({
		name: `codex-acp-j3-${provider}-interrupt`,
		steps: [
			{
				actions: [
					{ kind: 'text', text: longPrompt },
					{ kind: 'key', key: 'Enter' },
				],
				checkpoint: {
					scene: 'turn-running',
					require: [CODEX_0_165_TEXT.working],
					forbid: [CODEX_0_165_TEXT.interrupted],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Escape' }],
				checkpoint: {
					scene: 'turn-interrupted',
					require: [CODEX_0_165_TEXT.interrupted],
					forbid: [],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
		],
	});
}

/** The exact bytes one action writes to the PTY in the declared key protocol. */
export function encodeCodexAction(action: CodexAction, protocol: KeyProtocol): Buffer {
	if (action.kind === 'key') return encodeJourneyKey(action.key, protocol);
	return Buffer.from(
		action.kind === 'paste' ? `\u001b[200~${action.text}\u001b[201~` : action.text,
		'utf8',
	);
}

/** Whether one grid frame shows a checkpoint: every required text and no forbidden text. */
export function gridShowsCheckpoint(
	lines: readonly string[],
	checkpoint: CodexCheckpoint,
): boolean {
	const screen = lines.join('\n');
	return (
		checkpoint.require.every((text) => screen.includes(text)) &&
		checkpoint.forbid.every((text) => !screen.includes(text))
	);
}

/** Observed checkpoints must be exactly the journey's scenes, in its order. */
export function requireCheckpointSequence(
	observed: readonly string[],
	journey: CodexJourney,
): void {
	const expected = journey.steps.map((step) => step.checkpoint.scene);
	if (observed.length !== expected.length || observed.some((scene, i) => scene !== expected[i]))
		throw new Error(
			`checkpoint sequence differs: observed ${observed.join(',')}; expected ${expected.join(',')}`,
		);
}
