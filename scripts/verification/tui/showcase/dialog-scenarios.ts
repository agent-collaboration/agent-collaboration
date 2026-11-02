import { cellText } from '../terminal/terminal-grid.ts';
import { childTarget, PrerequisiteGap, requireObservation } from './contracts.ts';
import { observeFormSections, observeTypography } from './form-observations.ts';
import { observeCompleteHelp } from './help-projection.ts';
import type { ShowcaseRig } from './rig.ts';
import {
	currentRegistry,
	effectCount,
	escapeOverlay,
	openForm,
	sameFields,
	showHelpConvention,
	surfaceBounds,
	visibleTarget,
} from './scenario-helpers.ts';
import type { ScenarioVariant } from './scenario-variants.ts';

export async function livePrimary(rig: ShowcaseRig): Promise<void> {
	await openForm(rig);
	await rig.focus('draft');
	await rig.key('Left', rig.route(null, 'draft'));
	await rig.text('界', 'draft');
	const before = rig.state();
	await rig.key('Ctrl+R', rig.route('switch-primary'));
	const after = rig.state();
	sameFields(
		before,
		after,
		['focus', 'cursor', 'draft', 'scroll'],
		'live primary preserves form editing state',
	);
	requireObservation(
		before.overlays.at(-1)?.overlayId === after.overlays.at(-1)?.overlayId,
		'primary update retains overlay identity',
		after.overlays,
	);
	// Primary is the intentionally changed field of the same overlay identity.
	requireObservation(
		after.overlays.at(-1)?.primary === 'alternate-start',
		'current model exposes exactly the new primary',
		after.overlays,
	);
	const targets =
		after.hitMap?.targets.filter(
			(target) =>
				target.surface === after.surface &&
				['action:start', 'action:alternate-start'].includes(target.target),
		) ?? [];
	const markers = targets
		.flatMap((target) => target.cells)
		.filter(([column, row]) => {
			const cell = rig.grid().cells[row]?.[column];
			return cell !== undefined && cellText(cell).includes('●');
		});
	requireObservation(markers.length === 1, 'exactly one visible primary marker', markers);
	let fallbackCount = 0;
	if (rig.keys === 'legacy') {
		await rig.focus('action:alternate-start');
		await rig.key('Enter', rig.route('alternate-start', 'action:alternate-start'));
		fallbackCount = 1;
		requireObservation(
			effectCount(rig.state(), 'alternate-start') === 1 && effectCount(rig.state(), 'start') === 0,
			'legacy Tab plus Enter invokes only the current primary once',
			rig.state().effects,
		);
		await rig.capture('legacy-current-primary-fallback');
	}
	await rig.key('Ctrl+Enter', rig.route('alternate-start'));
	requireObservation(
		effectCount(rig.state(), 'alternate-start') === fallbackCount + 1 &&
			effectCount(rig.state(), 'start') === 0,
		'Ctrl+Enter invokes only the new primary once',
		rig.state().effects,
	);
	await rig.capture('new-primary');
	await escapeOverlay(rig);
}
export async function dialogSizing(rig: ShowcaseRig): Promise<void> {
	// Independent arithmetic: explicit child rows + title row + two border rows.
	// No production ScrollPosition or DialogFrame measurement supplies the expected height.
	const intrinsicFixtures = [
		['intrinsic-one', 1],
		['intrinsic-four', 4],
		['intrinsic-eight', 8],
	] as const;
	const observeIntrinsic = async (index: number): Promise<void> => {
		const fixture = intrinsicFixtures[index];
		if (fixture === undefined) return;
		const [action, bodyRows] = fixture;
		await rig.menuAction(action);
		const measured = surfaceBounds(rig.drawState());
		const actualHeight = measured.bottom - measured.top + 1;
		const expectedHeight = Math.min(bodyRows + 3, 30, rig.grid().rows - 2);
		requireObservation(
			actualHeight === expectedHeight,
			'arbitrary multiline, nested and gapped content has its independent intrinsic height',
			{ action, bodyRows, expectedHeight, actualHeight, measured },
		);
		rig.requireText('First');
		if (bodyRows + 3 <= rig.grid().rows - 2) rig.requireText('Last');
		await rig.capture(action);
		await escapeOverlay(rig);
		await observeIntrinsic(index + 1);
	};
	await observeIntrinsic(0);
	await openForm(rig);
	const bounds = surfaceBounds(rig.drawState());
	const height = bounds.bottom - bounds.top + 1;
	const contentUnobservable = !visibleTarget(rig.drawState(), 'draft');
	requireObservation(
		visibleTarget(rig.drawState(), 'action:close'),
		'content form includes actions',
		rig.state(),
	);
	requireObservation(
		height <= Math.min(30, rig.grid().rows - 2),
		'content form respects maximum and margins',
		{ height, grid: rig.state().viewport },
	);
	if (rig.grid().rows >= 30)
		requireObservation(height < 25, 'short content form has no viewport-filling blank filler', {
			height,
			bounds,
		});
	await rig.capture('content-form');
	await escapeOverlay(rig);
	await rig.key('F3', rig.route('percentage'));
	const percentage = surfaceBounds(rig.drawState());
	requireObservation(
		percentage.bottom - percentage.top + 1 > 2,
		'default-percentage dialog retains a substantial body',
		percentage,
	);
	await rig.capture('percentage-dialog');
	await escapeOverlay(rig);
	await rig.key('F5', rig.route('fill'));
	const fill = surfaceBounds(rig.drawState());
	requireObservation(
		fill.top === 1 && fill.bottom === rig.grid().rows - 2 && fill.right === rig.grid().columns - 3,
		'Fill respects explicit margins',
		fill,
	);
	await rig.capture('fill-dialog');
	await escapeOverlay(rig);
	if (contentUnobservable)
		throw new PrerequisiteGap(
			'sample explicit padding/shared label budget leaves no visible Content field; capped-body proof remains unobserved',
		);
}
export async function sectionsAndTypography(rig: ShowcaseRig): Promise<void> {
	await openForm(rig);
	let formGap: PrerequisiteGap | null = null;
	try {
		await observeFormSections(rig);
	} catch (error) {
		if (!(error instanceof PrerequisiteGap)) throw error;
		formGap = error;
		await rig.capture('sections-unobserved');
	}
	await escapeOverlay(rig);
	await rig.key('F5', rig.route('fill'));
	await observeTypography(rig);
	await escapeOverlay(rig);
	if (formGap !== null) throw formGap;
}
export async function screenHelp(rig: ShowcaseRig): Promise<void> {
	const before = rig.state();
	await rig.key('F1', rig.route('help'));
	rig.requireText('Help');
	rig.requireText('Main screen');
	const body = childTarget('action:help', 'help-body');
	const sourceRegistry = before.registries.screen;
	requireObservation(
		sourceRegistry !== null,
		'screen source registry is available for full Help census',
		before,
	);
	await observeCompleteHelp(rig, sourceRegistry);
	await showHelpConvention(rig, body);
	await rig.key('PageUp', rig.route(null, body));
	await rig.wheel(childTarget('action:help', 'help-scroll'), 'up');
	await rig.key('Home', rig.route(null, body));
	await rig.capture('screen-help');
	await escapeOverlay(rig);
	requireObservation(
		rig.state().focus === before.focus,
		'Help restores original focus',
		rig.state().focus,
	);
	await rig.menuAction('help');
	rig.requireText('Main screen');
	await escapeOverlay(rig);
}
export async function suspendedHelp(rig: ShowcaseRig): Promise<void> {
	await rig.menuAction('stage-update');
	await openForm(rig);
	await rig.focus('draft');
	await rig.key('Left', rig.route(null, 'draft'));
	await rig.text('界', 'draft');
	await rig.resize(rig.grid().columns, 12);
	await rig.wheel('dialog-scroll', 'down');
	requireObservation(
		(rig.state().scroll.dialog ?? 0) > 0,
		'form is actually scrolled before Help suspension',
		rig.state().scroll,
	);
	const before = rig.state();
	await rig.key('F1', rig.route('help'));
	requireObservation(
		rig.state().overlays.length === 1 &&
			rig.state().overlays[0]?.overlayId === before.overlays[0]?.overlayId,
		'dialog Help replaces one body on the same overlay identity',
		rig.state().overlays,
	);
	await rig.completeFixture();
	const base = rig.state().registries.overlays.at(-1)?.base;
	requireObservation(
		base?.actions.find((action) => action.actionId === 'start')?.title === 'Updated start 界' &&
			base.actions.find((action) => action.actionId === 'start')?.availability.kind ===
				'unavailable',
		'visible Help reads the current updated base actions',
		base,
	);
	await rig.capture('suspended-update-help');
	await escapeOverlay(rig);
	const after = rig.state();
	sameFields(
		before,
		after,
		['draft', 'cursor', 'choice', 'query', 'focus', 'scroll'],
		'closing Help restores current form state',
	);
	requireObservation(
		after.overlays[0]?.overlayId === before.overlays[0]?.overlayId &&
			after.overlays[0]?.primary === 'alternate-start' &&
			!after.startEnabled,
		'updated base survives closing Help',
		after,
	);
	await escapeOverlay(rig);
}
export async function dropdownHelp(rig: ShowcaseRig): Promise<void> {
	await rig.key('F2', rig.route('screen-choice-open'));
	const before = rig.state();
	await rig.key('F1', rig.route(), false);
	sameFields(before, rig.state(), ['choice'], 'actual screen-dropdown F1 never commits');
	await rig.capture('actual-dropdown-f1');
	await escapeOverlay(rig);
	await openForm(rig);
	await rig.focus('choice');
	await rig.key('Enter', rig.route('choice-open', 'choice'));
	const dialogBefore = rig.state();
	await rig.key('F1', rig.route(), false);
	sameFields(
		dialogBefore,
		rig.state(),
		['choice', 'draft'],
		'actual dialog-dropdown F1 never commits or changes draft',
	);
	await rig.capture('actual-dialog-dropdown-f1');
	throw new Error(
		'SC18 restoration oracle remains unobserved until the core owner pins F1 dropdown policy; actual route is retained',
	);
}
export async function modelLifecycle(
	rig: ShowcaseRig,
	transition: Extract<ScenarioVariant, { kind: 'lifecycle' }>['transition'],
): Promise<void> {
	if (transition === 'focus-replaced' || transition === 'help-focus-replaced') {
		await rig.menuAction('stage-focus-replace');
		await rig.menuAction('focus-sample');
		await rig.focus('draft');
		const before = rig.state();
		if (transition === 'help-focus-replaced') await rig.key('F1', rig.route('help'));
		await rig.completeFixture();
		if (transition === 'help-focus-replaced') await escapeOverlay(rig);
		const after = rig.state();
		requireObservation(
			after.focus === 'choice' &&
				!after.focusTargets.some((target) => target.targetId === 'draft') &&
				!visibleTarget(rig.drawState(), 'draft'),
			'arbitrary target replacement falls back to the first remaining target and removes its old hit geometry',
			after,
		);
		sameFields(
			before,
			after,
			['draft', 'committed'],
			'replacing a target never edits or commits its old value',
		);
		requireObservation(
			JSON.stringify(after.effects.slice(before.effects.length).map((effect) => effect.action)) ===
				JSON.stringify(transition === 'help-focus-replaced' ? ['completion-while-help'] : []),
			'replacement records only the declared fixture completion, never a user action',
			after.effects,
		);
		await rig.capture('lifecycle-' + transition);
		await rig.key('Enter', rig.route('choice-open', 'choice'));
		requireObservation(
			rig.state().overlays.length === 2,
			'replacement focus activates the new Choice control',
			rig.state(),
		);
		await escapeOverlay(rig);
		await escapeOverlay(rig);
		return;
	}
	await openForm(rig);
	const identity = rig.state().overlays.at(-1)?.overlayId;
	if (transition === 'close') {
		await rig.key('Ctrl+G', rig.route('update-close'));
		requireObservation(
			rig.state().overlays.every((overlay) => overlay.overlayId !== identity),
			'close inside updater keeps that identity closed',
			rig.state().overlays,
		);
	} else if (transition === 'help') {
		await rig.key('Ctrl+E', rig.route('update-help'));
		requireObservation(
			currentRegistry(rig.state()).scope === 'help',
			'Help opened inside updater survives the old update',
			rig.state(),
		);
		rig.requireText('Help');
		await rig.capture('lifecycle-help');
		await escapeOverlay(rig);
		await escapeOverlay(rig);
	} else {
		await escapeOverlay(rig);
		await rig.menuAction('late-completion');
		requireObservation(
			effectCount(rig.state(), 'late-effect') === 0 &&
				rig.state().overlays.every((overlay) => overlay.overlayId !== identity),
			'late completion neither runs an effect nor restores a closed identity',
			rig.state(),
		);
	}
}
