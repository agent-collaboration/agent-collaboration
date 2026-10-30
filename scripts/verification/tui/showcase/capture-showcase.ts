import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { PtyGridDriver } from '../pty-grid-driver.ts';
import { gridLines } from '../terminal-grid.ts';
import { renderOriginal } from '../terminal-original.ts';
import type { ShowcasePlan, ShowcaseState } from './contracts.ts';
export async function captureShowcase(options: {
	readonly directory: string;
	readonly driver: PtyGridDriver;
	readonly plan: ShowcasePlan;
	readonly state: ShowcaseState;
	readonly drawState: ShowcaseState;
	readonly name: string;
}): Promise<void> {
	const grid = options.driver.grid();
	writeFileSync(
		join(options.directory, options.name + '.draw-state.json'),
		JSON.stringify(options.drawState, null, 2),
	);
	writeFileSync(join(options.directory, options.name + '.ansi'), options.driver.bytes());
	writeFileSync(
		join(options.directory, options.name + '.grid.json'),
		JSON.stringify(grid, null, 2),
	);
	writeFileSync(
		join(options.directory, options.name + '.txt'),
		gridLines(options.driver.grid()).join('\n') + '\n',
	);
	writeFileSync(
		join(options.directory, options.name + '.state.json'),
		JSON.stringify(options.state, null, 2),
	);
	if (options.plan.chrome !== undefined)
		await renderOriginal({
			chrome: options.plan.chrome,
			output: options.directory,
			bytes: options.driver.bytes(),
			columns: grid.columns,
			rows: grid.rows,
			checkpoint: options.name,
			sourceHead: options.plan.source.head,
			binarySha256: options.plan.example.sha256,
			grid,
			resizes: options.driver.resizeHistory(),
		});
}
