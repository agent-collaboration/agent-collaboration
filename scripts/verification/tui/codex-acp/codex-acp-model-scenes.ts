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
	// The row's primary action also saves a default: "Model changed to … for this conversation",
	// or "Failed to save default model: …" with the endpoint's refusal
	// (app/event_dispatch.rs:2337-2357).
	default: { kind: 'key', key: 'Enter' },
};

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
	readonly expectedOutcome: readonly string[];
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
 * plain effort or none applies directly (model_popups.rs open_reasoning_popup). The outcome is the
 * declared text with no model or reasoning popup left open. The effective model and effort are
 * decided by the same-session observer.
 */
export function modelJourney(input: ModelJourneyInput): CodexJourney {
	if (input.cataloguePage.length === 0)
		throw new Error('model journey needs the declared catalogue page');
	if (input.effort !== null && ['max', 'ultra'].includes(input.effort.value))
		throw new Error('advanced efforts sit behind "More reasoning…" and are outside this journey');
	const header = CODEX_0_165_TEXT.modelPopupHeader;
	const targetRow = selectedRow({
		...input.target,
		label: input.target.displayName,
		isDefault: false,
	});
	const catalogueReady: CodexCheckpoint = {
		scene: 'model-catalogue-ready',
		require: [header, ...input.cataloguePage],
		forbid: [...input.staleRows],
		deadline_ms: UI_CHECKPOINT_MS,
		...(input.modelNavigation.length === 0 ? { selected_menu_row: targetRow } : {}),
	};
	const steps: CodexJourney['steps'][number][] = [
		{
			actions: [{ kind: 'text', text: '/model' }],
			checkpoint: {
				scene: 'model-command-ready',
				require: [CODEX_0_165_TEXT.modelCommandRow],
				forbid: [],
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
				forbid: [...input.staleRows],
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
				forbid: [],
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
					forbid: [],
					deadline_ms: UI_CHECKPOINT_MS,
					selected_menu_row: effortRow,
				},
			});
	}
	steps.push({
		actions: [MODEL_APPLY_ACTION[input.apply]],
		checkpoint: {
			scene: 'model-applied',
			require: [...input.expectedOutcome],
			forbid: [CODEX_0_165_TEXT.modelPopupHeader, CODEX_0_165_TEXT.effortPopupTitle],
			deadline_ms: input.outcomeDeadlineMs,
		},
	});
	return codexJourneySchema.parse({
		name: `codex-acp-j2-${input.provider}-model-${input.apply}`,
		steps,
	});
}
