/** Validate the published body's row witness separately from the text actually painted. */
import { z } from 'zod';

import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import { cellText } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import type { GridRectangle } from '../scenes/grid-scene-matcher.ts';
import type { QuotaLiveReadback } from './quota-live-fixture.ts';

const terminalRows = z.number().int().nonnegative().max(65535);
export const quotaBodyScrollSchema = z
	.strictObject({
		surface: z.string().min(1),
		mount: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
		publicationFrame: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
		offset: terminalRows,
		maxOffset: terminalRows,
		viewportHeight: terminalRows.positive(),
		contentHeight: terminalRows,
	})
	.refine(
		(value) =>
			value.offset <= value.maxOffset &&
			value.maxOffset === Math.max(0, value.contentHeight - value.viewportHeight),
		'body offset or maximum differs from its measured row bounds',
	);
export type QuotaBodyScroll = z.infer<typeof quotaBodyScrollSchema>;
const publishedBodySchema = z.object({
	frame: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
	targets: z.array(
		z.object({
			surface: z.string(),
			target: z.string(),
			kind: z.string(),
			cells: z.array(z.tuple([terminalRows, terminalRows])),
		}),
	),
});
export interface QuotaBodyViewport {
	readonly position: QuotaBodyScroll;
	readonly rows: readonly string[];
	readonly geometry: {
		readonly left: number;
		readonly right: number;
		readonly top: number;
		readonly bottom: number;
	};
}
export function readQuotaBodyViewport(options: {
	readonly readback: QuotaLiveReadback['readback'];
	readonly grid: TerminalGrid;
	readonly rectangle: GridRectangle;
	readonly surface: InputConsumptionExpectation['surface'];
}): QuotaBodyViewport {
	const { readback, grid, rectangle, surface } = options;
	const position = readback.bodyScroll;
	if (position === null) throw new Error('published quota body scroll witness absent');
	const published = publishedBodySchema.parse(readback.published);
	const expectedSurface =
		surface.kind === 'screen' ? 'Screen' : 'Overlay(OverlayId(' + surface.overlayId + '))';
	if (
		position.mount !== readback.mount ||
		position.publicationFrame !== published.frame ||
		position.surface !== expectedSurface
	)
		throw new Error('quota body scroll mount, frame or active surface differs');
	const candidates = published.targets.filter(
		(target) =>
			target.surface === expectedSurface && target.kind === 'ScrollArea' && target.cells.length > 0,
	);
	if (candidates.length !== 1)
		throw new Error('active quota body ScrollArea geometry is not unique');
	const target = candidates[0];
	if (target === undefined) throw new Error('quota body geometry absent');
	const columns = [...new Set(target.cells.map(([column]) => column))].toSorted(
		(first, second) => first - second,
	);
	const rows = [...new Set(target.cells.map(([, row]) => row))].toSorted(
		(first, second) => first - second,
	);
	const left = columns[0],
		right = columns.at(-1),
		top = rows[0],
		bottom = rows.at(-1);
	if (
		left === undefined ||
		right === undefined ||
		top === undefined ||
		bottom === undefined ||
		right <= left ||
		rows.length !== position.viewportHeight ||
		columns.some((column, index) => column !== left + index) ||
		rows.some((row, index) => row !== top + index) ||
		new Set(target.cells.map((cell) => cell.join(','))).size !== columns.length * rows.length ||
		left <= rectangle.left ||
		right >= rectangle.right ||
		top <= rectangle.top ||
		bottom >= rectangle.bottom ||
		right >= grid.columns ||
		bottom >= grid.rows
	)
		throw new Error('quota body published cells disagree with visible viewport');
	return {
		position,
		geometry: { left, right, top, bottom },
		rows: rows.map((row) =>
			(grid.cells[row] ?? []).slice(left, right).map(cellText).join('').trimEnd(),
		),
	};
}

/** End comes from offset==max; identical painted rows never terminate the census. */
export async function collectQuotaBodyRows(options: {
	readonly initial: QuotaBodyViewport;
	readonly step: (key: 'Up' | 'Down') => Promise<QuotaBodyViewport>;
}): Promise<readonly string[]> {
	const { initial, step } = options;
	if (initial.position.maxOffset > 256)
		throw new Error('quota body exceeds the bounded physical-row census');
	let current = initial;
	const move = async (key: 'Up' | 'Down'): Promise<void> => {
		const previous = current,
			next = await step(key);
		const before = previous.position,
			after = next.position;
		if (
			after.surface !== before.surface ||
			after.mount !== before.mount ||
			after.publicationFrame <= before.publicationFrame ||
			after.maxOffset !== before.maxOffset ||
			after.viewportHeight !== before.viewportHeight ||
			after.contentHeight !== before.contentHeight ||
			JSON.stringify(next.geometry) !== JSON.stringify(previous.geometry) ||
			after.offset !== before.offset + (key === 'Down' ? 1 : -1)
		)
			throw new Error('quota body step lacks a fresh exact one-row offset progression');
		const overlap = key === 'Down' ? previous.rows.slice(1) : previous.rows.slice(0, -1);
		const nextOverlap = key === 'Down' ? next.rows.slice(0, -1) : next.rows.slice(1);
		if (
			next.rows.length !== initial.rows.length ||
			JSON.stringify(overlap) !== JSON.stringify(nextOverlap)
		)
			throw new Error('quota body paint did not advance one physical row');
		current = next;
	};
	const rewind = async (): Promise<void> => {
		if (current.position.offset === 0) return;
		await move('Up');
		await rewind();
	};
	await rewind();
	const full = [...current.rows];
	const advance = async (): Promise<void> => {
		if (current.position.offset === current.position.maxOffset) return;
		await move('Down');
		const last = current.rows.at(-1);
		if (last === undefined) throw new Error('quota body paint empty');
		full.push(last);
		await advance();
	};
	await advance();
	const restore = async (): Promise<void> => {
		if (current.position.offset === initial.position.offset) return;
		await move('Up');
		if (
			JSON.stringify(current.rows) !==
			JSON.stringify(
				full.slice(
					current.position.offset,
					current.position.offset + current.position.viewportHeight,
				),
			)
		)
			throw new Error('quota body restoration differs from the indexed painted census');
		await restore();
	};
	await restore();
	if (JSON.stringify(current.rows) !== JSON.stringify(initial.rows))
		throw new Error('quota body entry paint was not restored');
	return full.slice(0, initial.position.contentHeight);
}
