/**
 * J2 `/model` scenes in the unchanged Codex 0.165 TUI: the refreshed catalogue, the selected model
 * and effort rows, and the apply key. Rows come from the actual bound and unbound model/list and
 * from Codex source at 360319d8; every scene stays unobserved until a declared run drives it.
 */
import { z } from 'zod';

import { CODEX_0_165_TEXT, UI_CHECKPOINT_MS, codexJourneySchema } from './codex-acp-scenes.ts';
import type { CodexAction, CodexCheckpoint, CodexJourney } from './codex-acp-scenes.ts';

/** How a chosen popup row is applied (chatwidget/model_popups.rs:144-160). */
export type CodexModelApply = 'session' | 'default';
const MODEL_APPLY_ACTION: Record<CodexModelApply, CodexAction> = {
	// session_model_selection.rs:53-98: plain `s` sends one thread/settings/update for this
	// session and writes no config ("Model changed to … for this session only",
	// app/model_defaults.rs:19-118; a refusal is "Failed to update thread settings: …").
	session: { kind: 'text', text: 's' },
	// A plain row's primary action updates the model and effort, then PersistModelSelection saves
	// them as a default (model_popups.rs:323-333). Success alone shows the regular info cell
	// codexSavedDefaultMessage builds; a refusal is "Failed to save default model: …"
	// (app/event_dispatch.rs:2563-2595). "… for this conversation" belongs only to the advanced
	// Max/Ultra path (event_dispatch.rs:2308-2358), which this journey refuses.
	default: { kind: 'key', key: 'Enter' },
};

/** The native model and effort values PersistModelSelection saves on a plain row's Enter. */
export interface CodexSavedDefault {
	// The native model/list row's model ID (model_popups.rs model_for_action), not its display name.
	readonly model: string;
	// The native effort value the action carries: the chosen popup row, or the model's only plain
	// effort (or its default when it lists none) when no popup opens (model_popups.rs:478-530).
	readonly effort: string;
}

// Printable ASCII without spaces: the info cell's width is then its length, so the viewport check
// is exact, and no value can smuggle a second word or a suffix into the message.
const NATIVE_VALUE = /^[\x21-\x7e]+$/u;

/**
 * The regular success cell after a saved default, as Codex 0.165 formats it
 * (app/event_dispatch.rs:2575-2584 with config_persistence.rs:1036-1047): the model ID, then the
 * effort's wire value, "default" for `none`, and no effort at all for a `codex-auto-` model.
 */
export function codexSavedDefaultMessage(saved: CodexSavedDefault): string {
	if (!NATIVE_VALUE.test(saved.model) || !NATIVE_VALUE.test(saved.effort))
		throw new Error('saved model and effort must be native printable values');
	if (saved.model.startsWith('codex-auto-')) return `Model changed to ${saved.model}`;
	return `Model changed to ${saved.model} ${saved.effort === 'none' ? 'default' : saved.effort}`;
}

/** Keys that only move a menu selection; the journey sends no other key while choosing. */
export const codexMenuKeySchema = z.strictObject({
	kind: z.literal('key'),
	key: z.enum(['Up', 'Down']),
});
export type CodexMenuKey = z.infer<typeof codexMenuKeySchema>;

const EFFORT_LABELS: Readonly<Record<string, string>> = {
	none: 'None',
	minimal: 'Minimal',
	low: 'Low',
	medium: 'Medium',
	high: 'High',
	xhigh: 'Extra high',
	max: 'Max',
	ultra: 'Ultra',
	persistent: 'Persistent',
};
/**
 * The label Codex 0.165 shows for a native effort value: known wire values map to fixed labels and
 * any other model-defined value is shown raw (openai_models.rs:84-97,148-166;
 * model_popups.rs reasoning_effort_label). A provider's own label is never the row label.
 */
export function codexEffortLabel(value: string): string {
	return EFFORT_LABELS[value] ?? value;
}

/** A numbered menu row the journey must see selected, with exactly these markers. */
export interface CodexMenuRow {
	readonly number: number;
	readonly label: string;
	readonly isDefault: boolean;
	readonly isCurrent: boolean;
}
/** The declared reasoning step: keys from the initial selection to the chosen effort row. */
export interface CodexEffortStep {
	readonly navigation: readonly CodexMenuKey[];
	readonly value: string;
	readonly number: number;
	readonly isDefault: boolean;
	readonly isCurrent: boolean;
}
export interface ModelJourneyInput {
	readonly provider: 'claude' | 'cursor';
	// Display names the refreshed catalogue shows on its visible page, from the bound model/list.
	readonly cataloguePage: readonly string[];
	// Rows only the cached bootstrap view shows; their presence means the refresh has not landed.
	readonly staleRows: readonly string[];
	readonly modelNavigation: readonly CodexMenuKey[];
	readonly target: {
		readonly displayName: string;
		readonly number: number;
		readonly isCurrent: boolean;
	};
	// Null when the model has one plain effort choice or none, which applies without a popup.
	readonly effort: CodexEffortStep | null;
	readonly apply: CodexModelApply;
	// `session` only: the declared outcome fragments, success or refusal.
	readonly expectedOutcome: readonly string[];
	// `default` only: what the action saves; its success cell is the whole outcome.
	readonly savedDefault: CodexSavedDefault | null;
	readonly outcomeDeadlineMs: number;
}

function selectedRow(row: CodexMenuRow): NonNullable<CodexCheckpoint['selected_menu_row']> {
	return { number: row.number, label: row.label, default: row.isDefault, current: row.isCurrent };
}

/**
 * J2 `/model` through the actual catalogue. /model draws the cached bootstrap catalogue first and
 * replaces it when FetchModels returns (chatwidget/model_popups.rs:16-40; app/event_dispatch.rs:
 * 2272-2300), so no key is sent until a frame shows the declared catalogue page with no stale row.
 * After each declared batch of menu keys, its final selection is proved by the selected numbered
 * row (list_selection_view.rs:742-791) before the next step's key; rows passed on the way are not
 * observed. A model with several plain efforts opens "Select Reasoning Level for <name>"; one
 * plain effort or none applies directly (model_popups.rs open_reasoning_popup). A session apply
 * ends on the declared text; a default apply ends on exactly one regular success cell for the
 * saved values, with no error cell, and no earlier step may already show that text. Either way no
 * model or reasoning popup is left open. The effective model and effort are decided by the
 * same-session observer, never by this cell.
 */
export function modelJourney(input: ModelJourneyInput): CodexJourney {
	if (input.cataloguePage.length === 0)
		throw new Error('model journey needs the declared catalogue page');
	if (input.effort !== null && ['max', 'ultra'].includes(input.effort.value))
		throw new Error('advanced efforts sit behind "More reasoning…" and are outside this journey');
	if (
		input.apply === 'session' &&
		(input.savedDefault !== null || input.expectedOutcome.length === 0)
	)
		throw new Error('a session apply declares its outcome text and saves no default');
	if (
		input.apply === 'default' &&
		(input.savedDefault === null || input.expectedOutcome.length > 0)
	)
		throw new Error('a default apply declares only the saved model and effort');
	if (
		input.savedDefault !== null &&
		input.effort !== null &&
		input.savedDefault.effort !== input.effort.value
	)
		throw new Error('the saved effort must be the chosen effort row');
	const saved = input.savedDefault === null ? null : codexSavedDefaultMessage(input.savedDefault);
	// Before the apply key, the success text must not already be on screen.
	const stale = saved === null ? [] : [saved];
	const header = CODEX_0_165_TEXT.modelPopupHeader;
	const targetRow = selectedRow({
		...input.target,
		label: input.target.displayName,
		isDefault: false,
	});
	const catalogueReady: CodexCheckpoint = {
		scene: 'model-catalogue-ready',
		require: [header, ...input.cataloguePage],
		forbid: [...input.staleRows, ...stale],
		deadline_ms: UI_CHECKPOINT_MS,
		...(input.modelNavigation.length === 0 ? { selected_menu_row: targetRow } : {}),
	};
	const steps: CodexJourney['steps'][number][] = [
		{
			actions: [{ kind: 'text', text: '/model' }],
			checkpoint: {
				scene: 'model-command-ready',
				require: [CODEX_0_165_TEXT.modelCommandRow],
				forbid: [...stale],
				deadline_ms: UI_CHECKPOINT_MS,
			},
		},
		{ actions: [{ kind: 'key', key: 'Enter' }], checkpoint: catalogueReady },
	];
	if (input.modelNavigation.length > 0)
		steps.push({
			actions: [...input.modelNavigation],
			checkpoint: {
				scene: 'model-row-selected',
				require: [header],
				forbid: [...input.staleRows, ...stale],
				deadline_ms: UI_CHECKPOINT_MS,
				selected_menu_row: targetRow,
			},
		});
	if (input.effort !== null) {
		const effortRow = selectedRow({ ...input.effort, label: codexEffortLabel(input.effort.value) });
		const effortHeader = `${CODEX_0_165_TEXT.effortPopupTitle} for ${input.target.displayName}`;
		steps.push({
			actions: [{ kind: 'key', key: 'Enter' }],
			checkpoint: {
				scene: 'reasoning-popup-open',
				require: [effortHeader],
				forbid: [...stale],
				deadline_ms: UI_CHECKPOINT_MS,
				...(input.effort.navigation.length === 0 ? { selected_menu_row: effortRow } : {}),
			},
		});
		if (input.effort.navigation.length > 0)
			steps.push({
				actions: [...input.effort.navigation],
				checkpoint: {
					scene: 'effort-selected',
					require: [effortHeader],
					forbid: [...stale],
					deadline_ms: UI_CHECKPOINT_MS,
					selected_menu_row: effortRow,
				},
			});
	}
	const popups = [CODEX_0_165_TEXT.modelPopupHeader, CODEX_0_165_TEXT.effortPopupTitle];
	steps.push({
		actions: [MODEL_APPLY_ACTION[input.apply]],
		checkpoint:
			saved === null
				? {
						scene: 'model-applied',
						require: [...input.expectedOutcome],
						forbid: popups,
						deadline_ms: input.outcomeDeadlineMs,
					}
				: {
						scene: 'model-applied',
						require: [saved],
						forbid: [...popups, CODEX_0_165_TEXT.errorPrefix],
						deadline_ms: input.outcomeDeadlineMs,
						info_line: saved,
					},
	});
	return codexJourneySchema.parse({
		name: `codex-acp-j2-${input.provider}-model-${input.apply}`,
		steps,
		...(saved === null ? {} : { whole_run_errors: 'forbid' }),
	});
}
