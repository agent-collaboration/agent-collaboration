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
export function changedMainMeasurement(
	before: Pick<ShowcaseState, 'mainScrollLayout'>,
	after: Pick<ShowcaseState, 'mainScrollLayout'>,
): boolean {
	const prior = before.mainScrollLayout,
		current = after.mainScrollLayout;
	return (
		prior !== null &&
		current !== null &&
		(prior.naturalHeight !== current.naturalHeight ||
			prior.viewportHeight !== current.viewportHeight ||
			prior.viewportTop !== current.viewportTop)
	);
}
export interface MainMeasurementFrame {
	readonly sequence: number;
	readonly layout: ShowcaseState['mainScrollLayout'];
}
/** A burst may have measured an intermediate input before its last input draw. */
export function mainMeasurementFollowUp(
	frames: readonly MainMeasurementFrame[],
	afterFrame: number,
): number {
	let previous: ShowcaseState['mainScrollLayout'] = null;
	let minimumFrame = 0;
	for (const frame of frames) {
		if (
			frame.sequence > afterFrame &&
			changedMainMeasurement({ mainScrollLayout: previous }, { mainScrollLayout: frame.layout })
		)
			minimumFrame = frame.sequence + 1;
		previous = frame.layout;
	}
	return minimumFrame;
}
