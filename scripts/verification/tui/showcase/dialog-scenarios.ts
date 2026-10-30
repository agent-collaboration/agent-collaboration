import { cellText } from '../terminal-grid.ts';
import { childTarget, requireObservation } from './contracts.ts';
import type { ShowcaseRig } from './rig.ts';
import {
	currentRegistry,
	effectCount,
	escapeOverlay,
	openForm,
	sameFields,
	surfaceBounds,
	visibleTarget,
} from './scenario-helpers.ts';

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
	await rig.key('Ctrl+Enter', rig.route('alternate-start'));
	requireObservation(
		effectCount(rig.state(), 'alternate-start') === 1 && effectCount(rig.state(), 'start') === 0,
		'Ctrl+Enter invokes only the new primary once',
		rig.state().effects,
	);
	await rig.capture('new-primary');
	await escapeOverlay(rig);
}
export async function dialogSizing(rig: ShowcaseRig): Promise<void> {
	await openForm(rig);
	const bounds = surfaceBounds(rig.drawState());
	const height = bounds.bottom - bounds.top + 1;
	requireObservation(
		visibleTarget(rig.drawState(), 'draft') && visibleTarget(rig.drawState(), 'action:close'),
		'content form includes body and actions',
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
}
export async function sectionsAndTypography(rig: ShowcaseRig): Promise<void> {
	await openForm(rig);
	const lines = rig.lines();
	for (const word of [
		'Draft connection',
		'Directory',
		'Option',
		'Tinted status',
		'Long section label',
		'Available',
		'Warning',
		'Error',
	])
		requireObservation(
			lines.some((line) => line.includes(word)),
			'declared form content is visibly drawn: ' + word,
			lines,
		);
	const directory = lines.findIndex((line) => line.includes('Directory'));
	const option = lines.findIndex((line) => line.includes('Option'));
	const status = lines.findIndex((line) => line.includes('Long section label'));
	const origin = (row: number, character: string): number =>
		rig.grid().cells[row]?.findIndex((cell) => cellText(cell) === character) ?? -1;
	const columns = [origin(directory, '/'), origin(option, 'S'), origin(status, 'A')];
	requireObservation(
		columns.every((column) => column === columns[0] && column > 0),
		'both sections share a physical value origin including selector gutter',
		columns,
	);
	const firstHeading = lines.findIndex((line) => line.includes('Draft connection'));
	const secondHeading = lines.findIndex((line) => line.includes('Tinted status'));
	requireObservation(
		directory === firstHeading + 1 && option === directory + 1 && secondHeading === option + 2,
		'one blank row separates sections, with no interior blank row',
		{ firstHeading, directory, option, secondHeading },
	);
	await rig.capture('sections-status');
	await escapeOverlay(rig);
	await rig.key('F5', rig.route('fill'));
	for (const role of [
		'Body',
		'Prominent',
		'Label',
		'Rule',
		'Secondary',
		'Disabled',
		'Selected',
		'Success',
		'Warning',
		'Error',
	])
		requireObservation(
			rig.lines().some((line) => line.includes('Public role ' + role)),
			'public typography role is visibly demonstrated ' + role,
			rig.lines(),
		);
	await rig.capture('typography-roles');
	await escapeOverlay(rig);
}
export async function screenHelp(rig: ShowcaseRig): Promise<void> {
	const before = rig.state();
	await rig.key('F1', rig.route('help'));
	rig.requireText('Help');
	rig.requireText('Main screen');
	const body = childTarget('action:help', 'help-body');
	await rig.key('End', rig.route(null, body));
	rig.requireText('Input conventions');
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
	transition: 'close' | 'help' | 'late',
): Promise<void> {
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
