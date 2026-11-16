/**
 * The O5 journeys that moved from D0 to D3: `/resume`, refused commands and the service-tier pick
 * (two windows are not built; see the skill reference). Each is a plain extension of the compare
 * journey's plan schema: the plan declares this target's texts, and the scenes require exactly
 * those. The 0.166.0 texts are not grounded here; a plan carries `DISCOVERY-*` placeholders until
 * one discovery run on real Codex (gpt-6-luna, low) fills them in. Built and unit-tested only.
 *
 * Their structure is judged from the wire tee's recording (codex-moved-evaluation.ts); the grid
 * frames only prove each step took effect on screen.
 */
import { z } from 'zod';

import { compareProviderSchema, scriptedSettingsSchema } from './codex-acp-compare-scenes.ts';
import { codexMenuKeySchema, modelJourney, modelPickFields } from './codex-acp-model-scenes.ts';
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

/** `/resume`: the picker's declared texts and the other thread the pick must open. */
export const resumeJourneySchema = z
	.strictObject({
		kind: z.literal('resume'),
		provider: compareProviderSchema,
		// The `/resume` row of the slash popup, as this target's CLI describes it.
		command_row: declaredTextSchema,
		picker: z.strictObject({
			header: declaredTextSchema,
			// Row labels the open picker shows, the target row among them.
			rows: z.array(declaredTextSchema).min(2),
			// Keys from the picker's initial selection to the target row.
			navigation: z.array(codexMenuKeySchema),
			target_row: declaredTextSchema,
		}),
		resumed: z.strictObject({
			// Lines only the target thread's own history draws; absent from the picker's text.
			history_markers: z.array(declaredTextSchema).min(1),
			// Lines only the thread the CLI launched on draws; they must not appear after the pick.
			other_thread_markers: z.array(declaredTextSchema).min(1),
		}),
		// The thread the pick must resume on the wire; never the thread the CLI launched on.
		expected_thread_id: z.uuid(),
		deadline_ms: deadlineSchema,
	})
	.refine((journey) => journey.picker.rows.includes(journey.picker.target_row), {
		message: 'the target row is one of the declared picker rows',
	})
	.refine(
		(journey) => {
			const pickerText = [journey.picker.header, ...journey.picker.rows].join('\n');
			return journey.resumed.history_markers.every((marker) => !pickerText.includes(marker));
		},
		{ message: 'a history marker appears in the picker text; the picker could satisfy it' },
	)
	.refine(
		(journey) =>
			journey.resumed.history_markers.every(
				(marker) => !journey.resumed.other_thread_markers.includes(marker),
			),
		{ message: 'a marker belongs to both threads' },
	);
export type ResumeJourneyDeclaration = z.infer<typeof resumeJourneySchema>;

/** `/resume`, the picker, the selected row, then the opened thread's own history. */
export function resumeJourney(input: ResumeJourneyDeclaration): CodexJourney {
	const { provider, command_row, picker, resumed, deadline_ms } = resumeJourneySchema.parse(input);
	const errors = [CODEX_0_165_TEXT.errorPrefix];
	const selected = CODEX_0_165_TEXT.selectedRowPrefix + picker.target_row;
	const steps: CodexJourney['steps'][number][] = [
		{
			actions: [{ kind: 'text', text: '/resume' }],
			checkpoint: {
				scene: 'resume-command-ready',
				require: [command_row],
				forbid: [picker.header, ...errors],
				deadline_ms: UI_CHECKPOINT_MS,
			},
		},
		{
			actions: [{ kind: 'key', key: 'Enter' }],
			checkpoint: {
				scene: 'resume-picker-open',
				require: [
					picker.header,
					...picker.rows,
					...(picker.navigation.length === 0 ? [selected] : []),
				],
				forbid: [...errors],
				deadline_ms,
			},
		},
	];
	if (picker.navigation.length > 0)
		steps.push({
			actions: [...picker.navigation],
			checkpoint: {
				scene: 'resume-row-selected',
				require: [picker.header, selected],
				forbid: [...errors],
				deadline_ms: UI_CHECKPOINT_MS,
			},
		});
	steps.push({
		actions: [{ kind: 'key', key: 'Enter' }],
		checkpoint: {
			scene: 'resume-opened',
			require: [...resumed.history_markers],
			forbid: [picker.header, ...resumed.other_thread_markers, ...errors],
			deadline_ms,
			empty_composer: true,
		},
	});
	return codexJourneySchema.parse({
		name: `codex-acp-o5-${provider}-resume`,
		steps,
		whole_run_errors: 'forbid',
	});
}

// One slash command as typed: `/name`, optionally with one argument text, on one composer row.
const slashCommandSchema = z
	.string()
	.min(2)
	.max(100)
	.regex(/^\/[a-z][a-z-]*(?: [^\p{Cc}]+)?$/u, 'a slash command with optional argument text')
	.refine((text) => text === text.trim(), 'the composer trims a submitted command');

/** Refused commands: each typed command and the one error cell its refusal draws. */
export const refusedJourneySchema = z
	.strictObject({
		kind: z.literal('refused'),
		provider: compareProviderSchema,
		commands: z
			.array(
				z.strictObject({
					command: slashCommandSchema,
					// The text after `■ ` in the whole error cell; it fits one grid row.
					refusal: declaredTextSchema,
				}),
			)
			.min(1)
			.max(6),
	})
	.refine(
		(journey) =>
			new Set(journey.commands.map((entry) => entry.command)).size === journey.commands.length,
		{ message: 'each command is declared once' },
	)
	.refine(
		(journey) => {
			const refusals = journey.commands.map((entry) => entry.refusal);
			return refusals.every((refusal, index) =>
				refusals.every((other, otherIndex) => index === otherIndex || !other.includes(refusal)),
			);
		},
		{ message: 'refusal texts are distinct and none contains another' },
	)
	.refine(
		(journey) =>
			journey.commands.every((entry) =>
				journey.commands.every((typed) => !typed.command.includes(entry.refusal)),
			),
		{ message: 'a refusal text appears in a typed command; an echo could satisfy it' },
	);
export type RefusedJourneyDeclaration = z.infer<typeof refusedJourneySchema>;

/**
 * Type each command and require its declared error cell with the composer empty again: a command
 * that ended the CLI never draws the empty composer. The whole-run error rule is scoped to
 * exactly these declared cells.
 */
export function refusedJourney(input: RefusedJourneyDeclaration): CodexJourney {
	const { provider, commands } = refusedJourneySchema.parse(input);
	return codexJourneySchema.parse({
		name: `codex-acp-o5-${provider}-refused`,
		steps: commands.flatMap((entry, index) => [
			{
				actions: [{ kind: 'text', text: entry.command }],
				checkpoint: {
					scene: `refused-${index + 1}-command-ready`,
					require: [CODEX_0_165_TEXT.composerPrompt + entry.command],
					forbid: [CODEX_0_165_TEXT.resumingSession, entry.refusal],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			{
				actions: [{ kind: 'key', key: 'Enter' }],
				checkpoint: {
					scene: `refused-${index + 1}-refused`,
					require: [CODEX_0_165_TEXT.errorPrefix + entry.refusal],
					forbid: [],
					deadline_ms: UI_CHECKPOINT_MS,
					error_line: entry.refusal,
					empty_composer: true,
				},
			},
		]),
		whole_run_errors: 'scoped',
	});
}

/** The service-tier pick that follows a session `/model` pick, and the tier the wire must carry. */
export const tierJourneySchema = z
	.strictObject({
		kind: z.literal('tier'),
		provider: compareProviderSchema,
		// The `/model` pick the tier step follows. A default apply would write the owner's config.
		model: z.strictObject({ ...modelPickFields, apply: z.literal('session') }),
		tier: z.strictObject({
			// Actions that open the tier choice after the model apply (a key, or `/model` and Enter).
			open: z.array(codexActionSchema).min(1),
			popup_header: declaredTextSchema,
			navigation: z.array(codexMenuKeySchema),
			row: z.strictObject({
				number: z.number().int().positive(),
				label: declaredTextSchema,
				is_current: z.boolean(),
				is_default: z.boolean(),
			}),
			apply: codexActionSchema,
			// The declared text the CLI shows once the tier is applied.
			outcome: z.array(declaredTextSchema).min(1),
			outcome_deadline_ms: deadlineSchema,
			// The tier the settings carry before the pick, and the one the pick must produce.
			initial_wire_value: declaredTextSchema.nullable(),
			wire_value: declaredTextSchema,
		}),
		// What the settings end with: the model and effort picked, and `tier.wire_value`.
		scripted_settings: scriptedSettingsSchema,
	})
	.refine((journey) => journey.scripted_settings.service_tier === journey.tier.wire_value, {
		message: 'the scripted settings end with the picked tier',
	})
	.refine((journey) => journey.tier.initial_wire_value !== journey.tier.wire_value, {
		message: 'the picked tier equals the one before the pick; the pick could not be seen',
	})
	.refine(
		(journey) =>
			journey.model.effort === null ||
			journey.scripted_settings.effort === journey.model.effort.value,
		{ message: 'the scripted effort is the effort row the /model pick chooses' },
	);
export type TierJourneyDeclaration = z.infer<typeof tierJourneySchema>;

/** The session `/model` pick, then the declared tier choice opened, selected and applied. */
export function tierJourney(input: TierJourneyDeclaration): CodexJourney {
	const { provider, model, tier } = tierJourneySchema.parse(input);
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
	const popups = [CODEX_0_165_TEXT.modelPopupHeader, CODEX_0_165_TEXT.effortPopupTitle];
	const row = {
		number: tier.row.number,
		label: tier.row.label,
		default: tier.row.is_default,
		current: tier.row.is_current,
	};
	return codexJourneySchema.parse({
		name: `codex-acp-o5-${provider}-tier`,
		steps: [
			...modelSteps,
			{
				actions: [...tier.open],
				checkpoint: {
					scene: 'tier-popup-open',
					require: [tier.popup_header],
					forbid: [CODEX_0_165_TEXT.errorPrefix, ...tier.outcome],
					deadline_ms: UI_CHECKPOINT_MS,
					...(tier.navigation.length === 0 ? { selected_menu_row: row } : {}),
				},
			},
			...(tier.navigation.length === 0
				? []
				: [
						{
							actions: [...tier.navigation],
							checkpoint: {
								scene: 'tier-row-selected',
								require: [tier.popup_header],
								forbid: [CODEX_0_165_TEXT.errorPrefix, ...tier.outcome],
								deadline_ms: UI_CHECKPOINT_MS,
								selected_menu_row: row,
							},
						},
					]),
			{
				actions: [tier.apply],
				checkpoint: {
					scene: 'tier-applied',
					require: [...tier.outcome],
					forbid: [tier.popup_header, ...popups, CODEX_0_165_TEXT.errorPrefix],
					deadline_ms: tier.outcome_deadline_ms,
				},
			},
		],
		whole_run_errors: 'forbid',
	});
}
