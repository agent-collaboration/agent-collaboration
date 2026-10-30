import type { TerminalGrid } from '../terminal-grid.ts';
/** Choose observation cells from the current published clipped map, never natural boxes. */
import { PrerequisiteGap } from './contracts.ts';
import type { ShowcaseState } from './contracts.ts';
export interface GridPoint {
	readonly column: number;
	readonly row: number;
}
export function pointForTarget(state: ShowcaseState, target: string): GridPoint {
	const region = state.hitMap?.targets.findLast(
		(candidate) =>
			candidate.surface === state.surface &&
			candidate.target === target &&
			candidate.kind === 'target',
	);
	const point = region?.cells.at(Math.floor((region?.cells.length ?? 0) / 2));
	if (point === undefined)
		throw new PrerequisiteGap('current complete draw has no visible target cells for ' + target);
	return { column: point[0], row: point[1] };
}
export function pointForScroll(state: ShowcaseState, target: string): GridPoint {
	const regions =
		state.hitMap?.targets.filter(
			(region) => region.surface === state.surface && region.kind === 'scrollArea',
		) ?? [];
	const owners = new Map<string, string>();
	for (const region of regions)
		for (const cell of region.cells) owners.set(cell.join(','), region.target);
	const point = regions
		.findLast((region) => region.target === target)
		?.cells.find((cell) => owners.get(cell.join(',')) === target);
	if (point === undefined)
		throw new PrerequisiteGap('no current unoccluded scroll cells for ' + target);
	return { column: point[0], row: point[1] };
}
export function pointOutsideSurface(state: ShowcaseState, grid: TerminalGrid): GridPoint {
	const surface = state.hitMap?.surfaces.find((region) => region.surface === state.surface);
	if (surface === undefined) throw new PrerequisiteGap('no current overlay footprint');
	const occupied = new Set(surface.cells.map((cell) => cell.join(',')));
	for (let row = grid.rows - 1; row >= 0; row -= 1)
		for (let column = grid.columns - 1; column >= 0; column -= 1)
			if (!occupied.has(column + ',' + row)) return { column, row };
	throw new PrerequisiteGap('overlay has no outside cell');
}
