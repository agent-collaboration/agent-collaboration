/** Controlled launches prove each surface uses its declared driver generation. */
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import type { PtyGridTarget } from './pty-grid-driver.ts';

const entries: readonly (readonly [PtyGridTarget['surface'], string])[] = [
	['quota', 'quota_storyboard::compiled_grid_terminal_observation'],
	['quota-filters', 'quota_storyboard::compiled_grid_terminal_observation'],
	['quota-effects', 'quota_storyboard::compiled_grid_terminal_observation'],
	['normal', 'quota_storyboard::compiled_grid_terminal_observation'],
	['hard', 'quota_storyboard::compiled_grid_terminal_observation'],
	['sessions', 'quota_storyboard::compiled_grid_terminal_observation'],
	['sessions-filters', 'quota_storyboard::compiled_grid_terminal_observation'],
	['public-sessions', 'quota_storyboard::compiled_public_sessions_grid_observation'],
	['showcase', 'quota_storyboard::compiled_grid_terminal_observation'],
];

test.each(entries)('%s launches only its declared exact driver entry', async (surface, entry) => {
	const repository = temporaryRoot();
	const observed = join(repository, 'entry.json');
	const executable = controlledExecutable(
		repository,
		'exact-entry-control',
		`
require('node:fs').writeFileSync(${JSON.stringify(observed)}, JSON.stringify(process.argv.slice(2)));
process.stdout.write('TUI_GRID READY 120 36 STREAM\\n');
process.stdin.on('data', () => { process.stdout.write('TUI_GRID STOPPED 0\\n'); process.exit(0); });`,
	);
	const driver = await PtyGridDriver.start({
		repository,
		driver: executable,
		surface,
		keys: 'legacy',
		evidence: repository,
	});
	try {
		expect(JSON.parse(readFileSync(observed, 'utf8'))).toEqual([
			entry,
			'--exact',
			'--ignored',
			'--nocapture',
			'--test-threads=1',
		]);
	} finally {
		await driver.stop();
	}
});
