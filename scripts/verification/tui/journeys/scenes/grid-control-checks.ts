/** Recheck deciding assertions against real wrong-scene grids and an empty core. */
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { CellData } from '@wterm/core';
import { z } from 'zod';

import { isMain, reportCliFailure } from '../../cli/cli-arguments.ts';
import {
	hardCaseNegatives,
	hardCaseShortNegatives,
	normalComponentNegatives,
	quotaNegativePairs,
} from '../../evidence/scene-contracts.ts';
import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../../terminal/terminal-grid.ts';
import { journeyCatalogue } from '../journey-catalogue.ts';
import { observeGridScene } from './grid-scene-matcher.ts';

export function negativeScenePairs(journeyName: string): readonly (readonly [string, string])[] {
	if (journeyName.startsWith('normal-')) return normalComponentNegatives;
	if (journeyName.startsWith('hard-'))
		return journeyName.endsWith('48x12') ? hardCaseShortNegatives : hardCaseNegatives;
	const surface = journeyName.startsWith('sessions-') ? 'sessions' : 'quota';
	const pairs: [string, string][] = [
		[`compact-${surface}-claude`, 'blank-startup'],
		[`compact-${surface}-claude`, `compact-${surface}-all`],
		[`compact-${surface}-cancelled`, `compact-${surface}-chooser`],
	];
	if (surface === 'quota')
		for (const [expected, wrong] of quotaNegativePairs)
			pairs.push([`compact-quota-${expected}`, `compact-quota-${wrong}`]);
	else
		for (const group of journeyCatalogue[journeyName]?.groups ?? [])
			if (group.checkpoint.startsWith('compact-sessions-details'))
				pairs.push([group.checkpoint, 'compact-sessions-codex-details']);
	if (journeyName === 'sessions-48x12')
		pairs.push(['compact-sessions-details-identity', 'compact-sessions-details-title']);
	return pairs;
}
const cellSchema = z
	.object({
		char: z.number(),
		chars: z.string().optional(),
		['fg']: z.number(),
		['bg']: z.number(),
		flags: z.number(),
		width: z.number().optional(),
		spacerHead: z.boolean().optional(),
		fgRgb: z.number().optional(),
		bgRgb: z.number().optional(),
		underlineStyle: z.enum(['none', 'single', 'double', 'curly', 'dotted', 'dashed']).optional(),
		underlineRgb: z.number().optional(),
		linkUri: z.string().optional(),
		linkId: z.string().optional(),
		linkKey: z.string().optional(),
	})
	.transform((value): CellData => ({
		char: value.char,
		['fg']: value.fg,
		['bg']: value.bg,
		flags: value.flags,
		...(value.chars === undefined ? {} : { chars: value.chars }),
		...(value.width === undefined ? {} : { width: value.width }),
		...(value.spacerHead === undefined ? {} : { spacerHead: value.spacerHead }),
		...(value.fgRgb === undefined ? {} : { fgRgb: value.fgRgb }),
		...(value.bgRgb === undefined ? {} : { bgRgb: value.bgRgb }),
		...(value.underlineStyle === undefined ? {} : { underlineStyle: value.underlineStyle }),
		...(value.underlineRgb === undefined ? {} : { underlineRgb: value.underlineRgb }),
		...(value.linkUri === undefined ? {} : { linkUri: value.linkUri }),
		...(value.linkId === undefined ? {} : { linkId: value.linkId }),
		...(value.linkKey === undefined ? {} : { linkKey: value.linkKey }),
	}));
const gridSchema = z.object({
	columns: z.number(),
	rows: z.number(),
	cells: z.array(z.array(cellSchema)),
	rowMetadata: z.array(
		z.object({ wrapsToNext: z.boolean(), continuesPrevious: z.boolean() }).nullable(),
	),
	cursor: z.object({ row: z.number(), col: z.number(), visible: z.boolean() }).passthrough(),
});
export function readTerminalGrid(path: string): TerminalGrid {
	return gridSchema.parse(JSON.parse(readFileSync(path, 'utf8')));
}
export async function checkGridControls(
	directory: string,
	journeyName: string,
): Promise<readonly unknown[]> {
	const journey = journeyCatalogue[journeyName];
	if (journey === undefined) throw new Error('unknown journey');
	const first = journey.groups[0];
	if (first === undefined) throw new Error('empty journey');
	const reference = readTerminalGrid(join(directory, `${first.checkpoint}.grid.json`));
	const core = await createTerminalCore(reference.columns, reference.rows);
	const blank = snapshotGrid(core);
	core.dispose();
	const results: unknown[] = [];
	for (const [expected, wrong] of negativeScenePairs(journeyName)) {
		const positive = observeGridScene(
			readTerminalGrid(join(directory, `${expected}.grid.json`)),
			expected,
		);
		if (!positive.passed)
			throw new Error(
				`positive failed while testing sensitivity: ${expected}; ${JSON.stringify(positive.missing)}`,
			);
		const rejected = observeGridScene(
			wrong === 'blank-startup' ? blank : readTerminalGrid(join(directory, `${wrong}.grid.json`)),
			expected,
		);
		if (rejected.passed) throw new Error(`wrong scene passed: ${expected} accepted ${wrong}`);
		results.push({
			expected,
			wrong,
			rejected: true,
			missing: rejected.missing,
			unexpected: rejected.unexpected,
		});
	}
	writeFileSync(join(directory, 'wrong-scene-controls.json'), JSON.stringify(results, null, 2));
	return results;
}
if (isMain(import.meta.url)) {
	const directory = process.argv[2],
		journey = process.argv[3];
	if (directory === undefined || journey === undefined)
		reportCliFailure(new Error('usage: grid-control-checks.ts CAPTURE_DIRECTORY JOURNEY'));
	else
		await checkGridControls(directory, journey)
			.then((rows) => process.stdout.write(`${rows.length} wrong-scene controls rejected\n`))
			.catch(reportCliFailure);
}
