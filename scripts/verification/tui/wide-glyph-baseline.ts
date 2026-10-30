/** U9 left-border baseline through the actual selectable storyboard fixture.
 * node scripts/verification/tui/wide-glyph-baseline.ts PLAN.json [--check]
 * The plan extends baselinePlanSchema with optional chrome (original PNG sink).
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { CellData } from '@wterm/core';
import { z } from 'zod';

import { baselinePlanSchema, checkBaselineTarget } from './baseline-journeys.ts';
import { isMain, planArgument, reportCliFailure, strictArguments } from './cli-arguments.ts';
import type { GridRectangle } from './grid-scene-matcher.ts';
import { findDialog } from './grid-scene-matcher.ts';
import { encodeJourneyKey } from './journey-key.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import type { KeyProtocol } from './pty-grid-driver.ts';
import { canonicalJson, sourceBinding } from './source-binding.ts';
import { cellText, gridCell, gridLines } from './terminal-grid.ts';
import { renderOriginal } from './terminal-original.ts';
export const wideGlyphPlanSchema = baselinePlanSchema.extend({ chrome: z.string().optional() });
type WideGlyphPlan = z.infer<typeof wideGlyphPlanSchema>;
interface BorderCellObservation {
	readonly glyph: string;
	readonly row: number;
	readonly column: number;
	readonly border: CellData;
	readonly gutter: CellData;
	readonly borderPresent: boolean;
}
export interface WideGlyphResult {
	readonly requirement: 'U9 / R10';
	readonly name: 'left-border loss';
	readonly title: 'New session' | 'Session details';
	readonly keys: KeyProtocol;
	readonly viewport: { readonly columns: 120; readonly rows: 36 };
	readonly rect: GridRectangle;
	readonly result: 'pass' | 'red';
	readonly expected: string;
	readonly observations: readonly BorderCellObservation[];
	readonly failedRows: readonly number[];
	readonly physicalTerminal: false;
}
async function observeDialog(
	plan: WideGlyphPlan,
	keys: KeyProtocol,
	title: 'New session' | 'Session details',
): Promise<WideGlyphResult> {
	const checkpoint = keys + '-' + (title === 'New session' ? 'new' : 'details');
	const directory = join(plan.output, checkpoint);
	mkdirSync(directory);
	const driver = await PtyGridDriver.start({
		repository: plan.repository,
		driver: plan.driver.path,
		surface: 'sessions-filters',
		keys,
		evidence: directory,
		storyboardVariant: 'wide-glyph-left-border',
	});
	try {
		const main = await driver.waitForGrid('wide storyboard entry', (grid) =>
			gridLines(grid).some((line) => line.includes('12345678')),
		);
		const positions = main.cells.flatMap((row, rowIndex) =>
			row.flatMap((cell, column) =>
				['📂', '✉️', '界'].includes(cellText(cell)) && cell.width === 2
					? [{ glyph: cellText(cell), row: rowIndex, column }]
					: [],
			),
		);
		const generation = driver.frameGeneration();
		await driver.send(encodeJourneyKey(title === 'New session' ? 'Ctrl+N' : 'F4', keys));
		const overlay = await driver.waitForGrid(
			title,
			(grid) => findDialog(grid, title) !== null,
			generation,
		);
		const rect = findDialog(overlay, title);
		if (rect === null) throw new Error('verification-design-gap: dialog entry absent');
		const boundary = positions.filter(
			(position) =>
				position.column === rect.left - 1 && position.row > rect.top && position.row < rect.bottom,
		);
		if (new Set(boundary.map((position) => position.glyph)).size !== 3)
			throw new Error(
				'verification-design-gap: all three glyphs not aligned on covered left-border rows',
			);
		const observations = boundary.map((position) => ({
			glyph: position.glyph,
			row: position.row,
			column: position.column,
			border: gridCell(overlay, position.row, rect.left),
			gutter: gridCell(overlay, position.row, rect.left - 1),
			borderPresent: ['│', '┃'].includes(cellText(gridCell(overlay, position.row, rect.left))),
		}));
		const failed = observations.filter((observation) => !observation.borderPresent);
		for (const [name, grid] of [
			['before', main],
			['overlay', overlay],
		] as const) {
			writeFileSync(join(directory, name + '.grid.json'), JSON.stringify(grid, null, 2));
			writeFileSync(join(directory, name + '.txt'), gridLines(grid).join('\n') + '\n');
		}
		writeFileSync(join(directory, 'overlay.ansi'), driver.bytes());
		if (plan.chrome !== undefined)
			await renderOriginal({
				chrome: plan.chrome,
				output: directory,
				bytes: driver.bytes(),
				grid: overlay,
				resizes: driver.resizeHistory(),
				columns: 120,
				rows: 36,
				checkpoint: 'overlay',
				sourceHead: plan.productSourceHead,
				binarySha256: plan.product.sha256,
			});
		const result = {
			requirement: 'U9 / R10',
			name: 'left-border loss',
			title,
			keys,
			viewport: { columns: 120, rows: 36 },
			rect,
			result: failed.length === 0 ? 'pass' : 'red',
			expected: 'every covered left-edge border cell survives; corner is not seeded',
			observations,
			failedRows: failed.map((observation) => observation.row),
			physicalTerminal: false,
		} satisfies WideGlyphResult;
		writeFileSync(join(directory, 'observation.json'), JSON.stringify(result, null, 2));
		return result;
	} finally {
		await driver.stop();
	}
}
export async function runWideGlyphBaseline(
	plan: WideGlyphPlan,
): Promise<readonly WideGlyphResult[]> {
	await checkBaselineTarget(plan);
	mkdirSync(plan.output);
	const source = sourceBinding(plan.repository);
	writeFileSync(join(plan.output, 'declared-target.json'), JSON.stringify(plan, null, 2));
	writeFileSync(join(plan.output, 'source-binding.json'), JSON.stringify(source, null, 2));
	const results: WideGlyphResult[] = [];
	const mode = async (keys: KeyProtocol): Promise<void> => {
		results.push(await observeDialog(plan, keys, 'New session'));
		results.push(await observeDialog(plan, keys, 'Session details'));
	};
	try {
		await mode('legacy');
		await mode('kitty');
		if (canonicalJson(sourceBinding(plan.repository)) !== canonicalJson(source))
			throw new Error('source changed during baseline');
		await checkBaselineTarget(plan);
		return results;
	} finally {
		writeFileSync(join(plan.output, 'wide-glyph-results.json'), JSON.stringify(results, null, 2));
	}
}
export async function wideGlyphMain(args: readonly string[]): Promise<void> {
	const { values, positionals } = strictArguments(
		args,
		{ help: { type: 'boolean' }, check: { type: 'boolean' } },
		true,
	);
	if (values.help) {
		process.stdout.write(
			'wide-glyph-baseline.ts PLAN.json [--check]: U9 New/Details left-border cells over 📂, ✉️ and 界 in both key modes at120x36. Requires the selectable fixture; a missing entry is a gap.\n',
		);
		return;
	}
	const plan = wideGlyphPlanSchema.parse(
		JSON.parse(readFileSync(planArgument(positionals), 'utf8')),
	);
	if (values.check) await checkBaselineTarget(plan);
	else await runWideGlyphBaseline(plan);
}
if (isMain(import.meta.url)) await wideGlyphMain(process.argv.slice(2)).catch(reportCliFailure);
