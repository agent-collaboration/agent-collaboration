/** Check painted cells against the fixture's declared ordinary-View intersections. */
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { cellText } from '../terminal/terminal-grid.ts';
import type { ShowcaseState } from './contracts.ts';
import { PrerequisiteGap, requireObservation } from './contracts.ts';
import type { ScenarioVariant } from './scenario-variants.ts';

export type ClippingFixture = Extract<ScenarioVariant, { kind: 'clipping' }>['fixture'];
interface ClipPoint {
	readonly column: number;
	readonly row: number;
}
interface ClippingObservation {
	readonly origin: ClipPoint;
	readonly negativePoints: readonly ClipPoint[];
}
type ClippingState = Pick<ShowcaseState, 'hitMap' | 'surface' | 'focusTargets'>;

function exactRow(
	cells: readonly (readonly [number, number])[],
	start: ClipPoint,
	width: number,
): boolean {
	return (
		cells.length === width &&
		new Set(cells.map((cell) => cell.join(','))).size === width &&
		cells.every(
			([column, row]) =>
				row === start.row && column >= start.column && column < start.column + width,
		)
	);
}

export function assertClippingPose(
	state: ClippingState,
	grid: TerminalGrid,
	fixture: ClippingFixture,
): ClippingObservation {
	const cellsFor = (targetId: string): readonly (readonly [number, number])[] =>
		state.hitMap?.targets
			.filter((target) => target.surface === state.surface && target.target === targetId)
			.flatMap((target) => target.cells) ?? [];
	const hitCells = cellsFor('action:clip-hit');
	const partialCells = cellsFor('action:clip-partial');
	if (hitCells.length === 0 || partialCells.length === 0)
		throw new PrerequisiteGap('positive clipping controls are not visible in this pose');
	const origin = {
		column: Math.min(...hitCells.map((cell) => cell[0])),
		row: Math.min(...hitCells.map((cell) => cell[1])),
	};
	const vertical = fixture.endsWith('vertical');
	// Button labels plus padding: Hit=5 cells; Partial=9. The six consumer
	// Views clip Partial to these independently declared intersections.
	const partialWidth = vertical
		? fixture === 'nested-vertical'
			? 6
			: 9
		: fixture === 'bordered-horizontal'
			? 4
			: 3;
	const partialOrigin = {
		column: origin.column + (vertical ? 0 : 6),
		row: origin.row + (vertical ? 1 : 0),
	};

	requireObservation(
		exactRow(hitCells, origin, 5) && exactRow(partialCells, partialOrigin, partialWidth),
		'published cells equal the independently declared clip intersection',
		{ hitCells, partialCells, fixture },
	);
	requireObservation(
		cellsFor('action:clip-hidden').length === 0 &&
			state.focusTargets.some((target) => target.targetId === 'action:clip-hidden'),
		'fully hidden target remains declared but has no visible cells',
		state,
	);
	const hitCell = grid.cells[origin.row]?.[origin.column + 1];
	const partialCell = grid.cells[partialOrigin.row]?.[partialOrigin.column + 1];
	requireObservation(
		hitCell !== undefined &&
			cellText(hitCell) === 'H' &&
			partialCell !== undefined &&
			cellText(partialCell) === 'P',
		'visible positive targets are really painted, not only registered',
		{ hitCell, partialCell },
	);
	const negativePoints = vertical
		? [
				{ column: origin.column + 1, row: origin.row + 2 },
				{ column: origin.column + 1, row: origin.row + 3 },
			]
		: [
				{ column: origin.column + 6 + partialWidth, row: origin.row },
				{ column: origin.column + 17, row: origin.row },
			];
	if (fixture === 'nested-horizontal')
		negativePoints.push({ column: origin.column + 7, row: origin.row + 1 });
	if (fixture === 'nested-vertical')
		negativePoints.push({ column: origin.column + 6, row: origin.row + 1 });
	return { origin, negativePoints };
}
