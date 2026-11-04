/**
 * Ordinary prompts in the unchanged Codex 0.165 TUI: one plain-text prompt and the one line only
 * its reply produces. The `prompt` journey submits it and waits for that reply; the refresh
 * journey keeps such a prompt running through /skills. Expected text comes from Codex source at
 * 360319d8, and the same-session observer's ordinary-reply readback decides the reply itself.
 */
import { z } from 'zod';

import { CODEX_0_165_TEXT, UI_CHECKPOINT_MS, codexJourneySchema } from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';

/**
 * One ordinary prompt typed as plain text: 1-100 characters on one composer row, with no leading
 * or trailing space because the composer trims what it submits (chat_composer.rs:3035), no control
 * character, and none of the composer's entry characters: `/` or `!` first (slash commands and
 * shell, :3099 and :3209), `$` or `@` anywhere (skill and file popups, sync_popups :3854).
 */
export const codexOrdinaryPromptSchema = z
	.string()
	.min(1)
	.max(100)
	.refine((text) => text === text.trim(), 'the composer trims a submitted prompt')
	.refine((text) => !/\p{Cc}/u.test(text), 'prompt carries a control character')
	.refine(
		(text) => !/^[/!]/u.test(text) && !/[$@]/u.test(text),
		'prompt opens a composer popup or mode',
	);

/**
 * An ordinary prompt and the completion its reply must produce: one distinctive line of at least
 * 8 characters that is absent from the prompt, from any other declared typed text, and from every
 * pinned Codex text, so neither an echo nor Codex's own drawing can satisfy it.
 */
export function requireOrdinaryReply(
	prompt: string,
	completion: string,
	otherTyped: readonly string[],
): void {
	codexOrdinaryPromptSchema.parse(prompt);
	if (completion.length < 8 || /\p{Cc}/u.test(completion))
		throw new Error('the completion is one distinctive line of at least 8 characters');
	if (
		[prompt, ...otherTyped, ...Object.values(CODEX_0_165_TEXT)].some((text) =>
			text.includes(completion),
		)
	)
		throw new Error(
			'the completion appears in typed or Codex-drawn text; an echo could satisfy it',
		);
}

export interface PromptJourneyInput {
	readonly provider: 'claude' | 'cursor';
	readonly prompt: string;
	// One line only the provider's reply produces; it is never typed or drawn by Codex itself.
	readonly completion: string;
	readonly replyDeadlineMs: number;
}

/**
 * The next ordinary prompt, such as the one after a /model selection: two deciding frames. First
 * the exact prompt drawn in the composer, with no stale completion, error cell or running turn on
 * screen and the resume landed, so Enter never races the typed text or a busy session. Then, after
 * Enter, the completion with an empty usable composer and no status row, interruption or new error
 * cell. A fast reply may skip any Working frame, so none is required.
 */
export function promptJourney(input: PromptJourneyInput): CodexJourney {
	requireOrdinaryReply(input.prompt, input.completion, []);
	return codexJourneySchema.parse({
		name: `codex-acp-j2-${input.provider}-prompt`,
		steps: [
			{
				actions: [{ kind: 'text', text: input.prompt }],
				checkpoint: {
					scene: 'prompt-composed',
					require: [CODEX_0_165_TEXT.composerPrompt + input.prompt],
					forbid: [
						input.completion,
						CODEX_0_165_TEXT.errorPrefix,
						CODEX_0_165_TEXT.working,
						CODEX_0_165_TEXT.resumingSession,
					],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: 'prompt-replied',
					require: [input.completion],
					// The composed frame showed no error cell, so any one here is new; the interrupted
					// notice is one too.
					forbid: [CODEX_0_165_TEXT.working, CODEX_0_165_TEXT.errorPrefix],
					deadline_ms: input.replyDeadlineMs,
					empty_composer: true,
				},
			},
		],
	});
}
