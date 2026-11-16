/**
 * O5 comparison journey: one action script, run identically against real Codex (the reference
 * through the Router's codex-local relay) and against a Router target with an ACP provider. The
 * steps are `send` (an ordinary prompt), `steer` (a long prompt, then steer text typed while the
 * turn runs), `model` (`/model` with a session apply), `status` (the footer's model and context
 * window) and the normal `/quit` every journey ends with. The structure of what the CLI exchanged
 * is judged from the wire tee's recording (verification/observations/providers/acp), never from
 * these grid frames: the frames only prove each step took effect on screen.
 *
 * Unobserved until a declared run drives it: that Enter while a turn runs submits a steer (it may
 * queue a message instead), and that the footer shows `footer_cells` for the declared status line.
 * A queued message would show as a second turn/start, which the cross-run rule reports.
 */
import { z } from 'zod';

import { declaredProviderSelectionSchema } from '../../../../verification/observations/mcp-control/observations/declared-model.ts';
import { modelJourney, modelPickFields } from './codex-acp-model-scenes.ts';
import { codexOrdinaryPromptSchema, requireOrdinaryReply } from './codex-acp-prompt-scenes.ts';
import { CODEX_0_165_TEXT, UI_CHECKPOINT_MS, codexJourneySchema } from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';

export const compareProviderSchema = z.enum(['claude', 'cursor', 'codex']);
export type CompareProvider = z.infer<typeof compareProviderSchema>;

/** What the script picked after `/model` for this target; the wire's settings must equal it. */
export const scriptedSettingsSchema = z.strictObject({
	model: z.string().min(1),
	effort: z.string().min(1).nullable(),
	// The tier the settings end with (null clears it). The script does not pick a tier itself.
	service_tier: z.string().min(1).nullable(),
});
export type ScriptedSettings = z.infer<typeof scriptedSettingsSchema>;

export const compareJourneySchema = z.strictObject({
	kind: z.literal('compare'),
	provider: compareProviderSchema,
	send: z.strictObject({
		prompt: codexOrdinaryPromptSchema,
		// One line only this target's reply produces, absent from everything typed or drawn by Codex.
		completion: z.string().min(8),
	}),
	steer: z.strictObject({
		// A prompt whose reply is long enough to still be running when the steer text is typed.
		prompt: codexOrdinaryPromptSchema,
		steer_text: codexOrdinaryPromptSchema,
		// The line only the reply to the steered turn produces.
		completion: z.string().min(8),
	}),
	// The `/model` pick for this target. A default apply would write the owner's Codex config.
	model: z.strictObject({ ...modelPickFields, apply: z.literal('session') }),
	scripted_settings: scriptedSettingsSchema,
	// A Router target's selected model and options, checked again just before the first Enter
	// (Codex resume can apply settings to the Session). Null only for the real-Codex reference.
	initial_selection: declaredProviderSelectionSchema.nullable(),
	// Literal footer text for this target: its model and its context window.
	status: z.strictObject({ footer_cells: z.array(z.string().min(1)).min(2) }),
	reply_deadline_ms: z.number().int().positive().max(600_000),
});
export type CompareJourneyDeclaration = z.infer<typeof compareJourneySchema>;

export const COMPARE_ACTION_SCRIPT = ['send', 'steer', 'model', 'status', 'quit'] as const;

/** The compare journey's grid steps; `/quit` follows through the shared exit like any journey. */
export function compareJourney(input: CompareJourneyDeclaration): CodexJourney {
	const { send, steer, model, status } = compareJourneySchema.parse(input);
	const typed = [send.prompt, steer.prompt, steer.steer_text];
	requireOrdinaryReply(send.prompt, send.completion, [
		steer.prompt,
		steer.steer_text,
		steer.completion,
	]);
	requireOrdinaryReply(steer.prompt, steer.completion, [
		send.prompt,
		steer.steer_text,
		send.completion,
	]);
	if (status.footer_cells.some((cell) => typed.some((text) => text.includes(cell))))
		throw new Error('a footer cell appears in typed text; an echo could satisfy it');
	const modelSteps = modelJourney({
		provider: 'claude',
		cataloguePage: model.catalogue_page,
		staleRows: model.stale_rows,
		modelNavigation: model.model_navigation,
		target: {
			displayName: model.target.display_name,
			number: model.target.number,
			isCurrent: model.target.is_current,
		},
		effort:
			model.effort === null
				? null
				: {
						navigation: model.effort.navigation,
						value: model.effort.value,
						number: model.effort.number,
						isDefault: model.effort.is_default,
						isCurrent: model.effort.is_current,
					},
		apply: 'session',
		expectedOutcome: model.expected_outcome,
		savedDefault: null,
		outcomeDeadlineMs: model.outcome_deadline_ms,
	}).steps;
	const common = [CODEX_0_165_TEXT.errorPrefix];
	return codexJourneySchema.parse({
		name: `codex-acp-o5-${input.provider}-compare`,
		steps: [
			{
				actions: [{ kind: 'text', text: send.prompt }],
				checkpoint: {
					scene: 'send-composed',
					require: [CODEX_0_165_TEXT.composerPrompt + send.prompt],
					forbid: [
						send.completion,
						CODEX_0_165_TEXT.working,
						CODEX_0_165_TEXT.resumingSession,
						...common,
					],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: 'send-replied',
					require: [send.completion],
					forbid: [CODEX_0_165_TEXT.working, ...common],
					deadline_ms: input.reply_deadline_ms,
					empty_composer: true,
				},
			},
			{
				actions: [{ kind: 'text', text: steer.prompt }],
				checkpoint: {
					scene: 'steer-prompt-composed',
					require: [CODEX_0_165_TEXT.composerPrompt + steer.prompt],
					forbid: [steer.completion, CODEX_0_165_TEXT.working, ...common],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: 'steer-turn-running',
					require: [],
					running_turn: true,
					forbid: [steer.completion, ...common],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				// Typed while the turn runs: a reply that already finished fails this frame loudly.
				actions: [{ kind: 'text', text: steer.steer_text }],
				checkpoint: {
					scene: 'steer-composed',
					require: [CODEX_0_165_TEXT.composerPrompt + steer.steer_text],
					// Still running (C51, C65): Working, the in-turn hint, or the wire's running turn.
					running_turn: true,
					forbid: [steer.completion, ...common],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: 'steer-replied',
					require: [steer.completion],
					forbid: [CODEX_0_165_TEXT.working, ...common],
					deadline_ms: input.reply_deadline_ms,
					empty_composer: true,
				},
			},
			...modelSteps,
			{
				actions: [],
				checkpoint: {
					scene: 'status-footer',
					require: [...status.footer_cells],
					forbid: [CODEX_0_165_TEXT.modelPopupHeader, CODEX_0_165_TEXT.effortPopupTitle, ...common],
					deadline_ms: UI_CHECKPOINT_MS,
					empty_composer: true,
				},
			},
		],
		whole_run_errors: 'forbid',
	});
}
