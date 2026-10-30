/** Fixture initialization uses measured public controls; their follow-up draw is causal. */
import type { ShowcaseState } from './contracts.ts';
export function viewScope(state: ShowcaseState): string | null {
	return state.surface === 'screen'
		? (state.registries.screen?.scope ?? null)
		: (state.registries.overlays.find((entry) => entry.surface === state.surface)?.current.scope ??
				null);
}
export function needsMeasurementDraw(before: ShowcaseState, after: ShowcaseState): boolean {
	const top = after.overlays.at(-1);
	const newOverlay =
		top !== undefined && !before.overlays.some((overlay) => overlay.overlayId === top.overlayId);
	const changedBody =
		before.surface === after.surface &&
		viewScope(before) !== viewScope(after) &&
		(viewScope(after) === 'help' ||
			after.registries.overlays
				.at(-1)
				?.current.actions.some((action) => action.actionId === 'choice-open') === true);
	return newOverlay || changedBody || before.viewport[1] !== after.viewport[1];
}
