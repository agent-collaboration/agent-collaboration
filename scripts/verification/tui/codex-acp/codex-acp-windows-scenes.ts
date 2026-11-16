/**
 * O5 two windows (Verification Design #O5-windows): window A sends two ordinary prompts while
 * window B, a second concurrent CLI on the same target, attaches to the same thread and only
 * watches. Turn 1 materializes the thread: real Codex writes a thread's rollout only at its first
 * user message, so a second CLI can resume it only after that (assignment Correction 32). B attaches
 * after turn 1's reply, and turn 2 is the turn the rules judge, with B attached live. The Router
 * half runs the same script, so the two halves stay symmetric. Window B's single step requires
 * turn 2's completion line on B's own grid. What each window exchanged is judged from the wire
 * tee's per-connection recording (codex-window-evaluation.ts, rules w1-w4, over the turns A starts
 * after B connects), never from these frames. Built and unit-tested only; D3 runs it live.
 */
import { z } from 'zod';

import { compareProviderSchema } from './codex-acp-compare-scenes.ts';
import { codexOrdinaryPromptSchema, requireOrdinaryReply } from './codex-acp-prompt-scenes.ts';
import { CODEX_0_165_TEXT, UI_CHECKPOINT_MS, codexJourneySchema } from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';

const windowsTurnSchema = z.strictObject({
	// Typed and submitted in window A only.
	prompt: codexOrdinaryPromptSchema,
	// One line only this turn's reply produces.
	completion: z.string().min(8),
});

export const windowsJourneySchema = z
	.strictObject({
		kind: z.literal('windows'),
		provider: compareProviderSchema,
		// Turn 1: materializes the thread before window B attaches; not judged by w1-w4's mirror rules.
		materialize: windowsTurnSchema,
		// Turn 2: the judged turn; window B must draw its completion too.
		send: windowsTurnSchema,
		reply_deadline_ms: z.number().int().positive().max(600_000),
	})
	.refine(
		(journey) =>
			journey.materialize.completion !== journey.send.completion &&
			!journey.send.prompt.includes(journey.materialize.completion) &&
			!journey.materialize.prompt.includes(journey.send.completion),
		'the two turns need distinct completions that neither prompt can echo',
	);
export type WindowsJourneyDeclaration = z.infer<typeof windowsJourneySchema>;

/** Scene after which window B attaches: turn 1 has replied, so the thread's rollout exists. */
export const WINDOW_B_ATTACH_SCENE = 'window-a-first-replied';

export interface WindowsJourneys {
	readonly windowA: CodexJourney;
	readonly windowB: CodexJourney;
}

/** Window A runs two turns; window B, attached after the first, must show the second's reply. */
export function windowsJourneys(input: WindowsJourneyDeclaration): WindowsJourneys {
	const { provider, materialize, send, reply_deadline_ms } = windowsJourneySchema.parse(input);
	requireOrdinaryReply(materialize.prompt, materialize.completion, []);
	requireOrdinaryReply(send.prompt, send.completion, []);
	const errors = [CODEX_0_165_TEXT.errorPrefix];
	return {
		windowA: codexJourneySchema.parse({
			name: `codex-acp-o5-${provider}-windows`,
			steps: [
				{
					actions: [{ kind: 'text', text: materialize.prompt }],
					checkpoint: {
						scene: 'window-a-first-composed',
						require: [CODEX_0_165_TEXT.composerPrompt + materialize.prompt],
						forbid: [materialize.completion, CODEX_0_165_TEXT.working, ...errors],
						deadline_ms: UI_CHECKPOINT_MS,
					},
				},
				{
					actions: [{ kind: 'key', key: 'Enter' }],
					checkpoint: {
						scene: WINDOW_B_ATTACH_SCENE,
						require: [materialize.completion],
						forbid: [CODEX_0_165_TEXT.working, send.completion, ...errors],
						deadline_ms: reply_deadline_ms,
						empty_composer: true,
					},
				},
				{
					actions: [{ kind: 'text', text: send.prompt }],
					checkpoint: {
						scene: 'window-a-composed',
						require: [CODEX_0_165_TEXT.composerPrompt + send.prompt],
						forbid: [send.completion, CODEX_0_165_TEXT.working, ...errors],
						deadline_ms: UI_CHECKPOINT_MS,
					},
				},
				{
					actions: [{ kind: 'key', key: 'Enter' }],
					checkpoint: {
						scene: 'window-a-replied',
						require: [send.completion],
						forbid: [CODEX_0_165_TEXT.working, ...errors],
						deadline_ms: reply_deadline_ms,
						empty_composer: true,
					},
				},
			],
			whole_run_errors: 'forbid',
		}),
		windowB: codexJourneySchema.parse({
			name: `codex-acp-o5-${provider}-windows-b`,
			steps: [
				{
					actions: [],
					checkpoint: {
						scene: 'window-b-sees-reply',
						require: [send.completion],
						// B typed nothing, so neither the prompt nor an error may be on its screen.
						forbid: [...errors],
						deadline_ms: reply_deadline_ms,
					},
				},
			],
			whole_run_errors: 'forbid',
		}),
	};
}
