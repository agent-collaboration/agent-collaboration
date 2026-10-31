/**
 * J2 read-only information commands in the unchanged Codex 0.165 TUI: `/hooks` and `/mcp` with no
 * arguments, their default inventory flows (chatwidget/slash_dispatch.rs:539-541 and :606-608).
 * No other slash text, argument, login or option is driven. Expected text comes from Codex source
 * at 360319d8; every scene stays unobserved until a declared run drives it.
 */
import { z } from 'zod';

import { CODEX_0_165_TEXT, UI_CHECKPOINT_MS, codexJourneySchema } from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';

/** The only information commands this journey drives. */
export const codexInformationCommandSchema = z.enum(['hooks', 'mcp']);
export type CodexInformationCommand = z.infer<typeof codexInformationCommandSchema>;

interface InformationSurface {
	readonly description: string;
	// The whole error line Codex draws when the endpoint refuses the inventory request.
	readonly refusal: string;
	// Text drawn only while the request is still pending.
	readonly pending: readonly string[];
}
const INFORMATION_SURFACES: Readonly<Record<CodexInformationCommand, InformationSurface>> = {
	// /hooks sends hooks/list and opens its browser only on success (chatwidget/hooks.rs:11-31).
	hooks: {
		description: CODEX_0_165_TEXT.hooksCommandRow,
		refusal: CODEX_0_165_TEXT.hooksLoadFailed,
		pending: [],
	},
	// /mcp draws a loading cell, sends mcpServerStatus/list, and replaces the cell with the
	// inventory or an error (chatwidget.rs:1620-1632; app/background_requests.rs:729-760).
	mcp: {
		description: CODEX_0_165_TEXT.mcpCommandRow,
		refusal: CODEX_0_165_TEXT.mcpLoadFailed,
		pending: [CODEX_0_165_TEXT.mcpLoading],
	},
};

/**
 * Type the command, wait for its own selected slash-popup row, then Enter. The endpoint refuses
 * the inventory request, so Codex draws one new error cell, keeps the composer empty and usable,
 * and the journey then exits through `/quit`. The ready frame forbids that error line, so an error
 * left by earlier input in this TUI cannot pass, and it waits for the resumed session. The cell
 * holds only Codex's own wrapper text: any refusal of the request draws the same line, so the
 * endpoint's code and message are never on screen and this journey never decides them.
 */
export function informationJourney(
	provider: 'claude' | 'cursor',
	command: CodexInformationCommand,
): CodexJourney {
	const surface = INFORMATION_SURFACES[codexInformationCommandSchema.parse(command)];
	const slashRow = { command, description: surface.description };
	return codexJourneySchema.parse({
		name: `codex-acp-j2-${provider}-${command}`,
		steps: [
			{
				actions: [{ kind: 'text', text: `/${command}` }],
				checkpoint: {
					scene: `${command}-command-ready`,
					require: [surface.description],
					forbid: [surface.refusal, CODEX_0_165_TEXT.resumingSession],
					deadline_ms: UI_CHECKPOINT_MS,
					selected_slash_row: slashRow,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: `${command}-refused`,
					require: [surface.refusal],
					// The empty composer also proves the popup closed and the command dispatched: the slash
					// popup is drawn only while the first line holds a `/` command name being edited
					// (chat_composer.rs:3966-3990).
					forbid: [...surface.pending],
					deadline_ms: UI_CHECKPOINT_MS,
					error_line: surface.refusal,
					empty_composer: true,
				},
			},
		],
	});
}
