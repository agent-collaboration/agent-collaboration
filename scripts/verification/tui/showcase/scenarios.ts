import {
	comboPickCancel,
	comboOptionUpdates,
	comboUnavailable,
	compoundDropdown,
	anchoredResize,
} from './combo-scenarios.ts';
/** Scenario IDs and declared obligations are retained from the Advisor's outline. */
import type { ScenarioId } from './contracts.ts';
import { PrerequisiteGap, requireObservation } from './contracts.ts';
import { outsideClicks, coveredSurfaces, wideGlyphCoverage } from './coverage-scenarios.ts';
import {
	livePrimary,
	dialogSizing,
	sectionsAndTypography,
	screenHelp,
	suspendedHelp,
	dropdownHelp,
	modelLifecycle,
} from './dialog-scenarios.ts';
import { inputPhases } from './input-scenarios.ts';
import { rowPaging, contentScroll, clippedGeometry } from './list-scenarios.ts';
import type { ShowcaseRig } from './rig.ts';
import type { ScenarioVariant } from './scenario-variants.ts';
import {
	toolbarDropOff,
	menuProjections,
	segmentedSettings,
	fieldEditing,
	mainActionMeanings,
	escapeAndQuit,
	liveActions,
} from './screen-scenarios.ts';
export const scenarioTitles: Readonly<Record<ScenarioId, string>> = {
	SC01: 'Entry and ordinary layout',
	SC02: 'Toolbar drop-off',
	SC03: 'Menu opening and projections',
	SC04: 'SegmentedSwitch and checked settings',
	SC05: 'TextField and FilterField',
	SC06: 'Main-screen action meanings',
	SC07: 'Main Esc ladder and Ctrl+C',
	SC08: 'Live titles and availability',
	SC09: 'Live primary change',
	SC10: 'Content, percentage and Fill sizing',
	SC11: 'Sections, fields, status and typography',
	SC12: 'ComboBox pick and cancel',
	SC13: 'ComboBox option updates',
	SC14: 'ComboBox unavailable and no match',
	SC15: 'Compound dropdown keys',
	SC16: 'Screen Help',
	SC17: 'Dialog Help and suspended updates',
	SC18: 'Help from a dropdown',
	SC19: 'Outside clicks and focus restoration',
	SC20: 'RowPaging list',
	SC21: 'ContentScroll list and marker offset',
	SC22: 'Real clipped geometry',
	SC23: 'Anchored dropdown resize',
	SC24: 'Covered surfaces',
	SC25: 'Wide-glyph coverage',
	SC26: 'Model update versus lifecycle',
	SC27: 'Input phases and receipts',
};
export async function driveScenario(
	scenarioId: ScenarioId,
	rig: ShowcaseRig,
	variant: ScenarioVariant = { kind: 'ordinary' },
): Promise<void> {
	if (scenarioId === 'SC07') {
		if (variant.kind !== 'quit') throw new Error('SC07 requires its declared Quit variant');
		await escapeAndQuit(rig);
		if (variant.surface === 'menu') await rig.menu();
		else if (variant.surface === 'dropdown') await rig.key('F2', rig.route('screen-choice-open'));
		else if (variant.surface === 'dialog') await rig.key('Ctrl+N', rig.route('form'));
		return;
	}
	if (scenarioId === 'SC25') {
		if (variant.kind !== 'wide') throw new Error('SC25 requires its declared glyph/scene');
		await wideGlyphCoverage(rig, variant);
		return;
	}
	if (scenarioId === 'SC26') {
		if (variant.kind !== 'lifecycle') throw new Error('SC26 requires its declared transition');
		await modelLifecycle(rig, variant.transition);
		return;
	}
	const screenDrivers = {
		SC02: toolbarDropOff,
		SC03: menuProjections,
		SC04: segmentedSettings,
		SC05: fieldEditing,
		SC06: mainActionMeanings,
		SC07: escapeAndQuit,
		SC08: liveActions,
		SC09: livePrimary,
		SC10: dialogSizing,
		SC11: sectionsAndTypography,
		SC12: comboPickCancel,
		SC13: comboOptionUpdates,
		SC14: comboUnavailable,
		SC15: compoundDropdown,
		SC16: screenHelp,
		SC17: suspendedHelp,
		SC18: dropdownHelp,
		SC19: outsideClicks,
		SC20: rowPaging,
		SC21: contentScroll,
		SC22: clippedGeometry,
		SC23: anchoredResize,
		SC24: coveredSurfaces,
		SC27: inputPhases,
	};
	if (scenarioId in screenDrivers) {
		const driver = Object.entries(screenDrivers).find(([name]) => name === scenarioId)?.[1];
		if (driver !== undefined) {
			await driver(rig);
			return;
		}
	}
	if (scenarioId === 'SC01') {
		const initial = rig.state();
		requireObservation(
			initial.selection === 'sample-00' && initial.focus === 'sample-list',
			'first stable list key has keyboard focus',
			initial,
		);
		requireObservation(
			initial.focusTargets
				.filter((target) => target.targetId.includes(':row:'))
				.every((target) => target.role === 'pointerOnly'),
			'pointer rows never become Tab stops',
			initial.focusTargets,
		);
		rig.requireText('Menu');
		const enabled = initial.focusTargets
			.filter((target) => target.role === 'focusable' && target.eligible)
			.map((target) => target.targetId);
		const seen: string[] = [];
		const traverse = async (remaining: number): Promise<void> => {
			if (remaining === 0) return;
			await rig.key('Tab');
			const focus = rig.state().focus;
			requireObservation(
				focus !== null && !focus.includes(':row:'),
				'Tab reaches a control, not a pointer row',
				focus,
			);
			seen.push(focus);
			await traverse(remaining - 1);
		};
		await traverse(enabled.length);
		requireObservation(
			seen.at(-1) === initial.focus && new Set(seen).size === enabled.length,
			'Tab traverses enabled controls once and wraps',
			{ enabled, seen },
		);
		await rig.key('Shift+Tab');
		await rig.key('Tab');
		requireObservation(
			rig.state().focus === initial.focus,
			'reverse traversal restores original focus',
			rig.state().focus,
		);
		await rig.capture('entry');
		return;
	}
	throw new PrerequisiteGap('scenario driver implementation pending: ' + scenarioId);
}
