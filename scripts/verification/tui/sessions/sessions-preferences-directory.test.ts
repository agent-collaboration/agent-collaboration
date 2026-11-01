import { chmodSync, mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { sessionsPreferencesEnvironment } from './sessions-preferences-directory.ts';

test('preferences are explicit, private, and may reopen only inside the owned lane tmp', () => {
	const repository = temporaryRoot();
	const directory = join(repository, 'tmp', 'run', 'preferences');
	mkdirSync(directory, { recursive: true, mode: 0o700 });
	expect(sessionsPreferencesEnvironment(undefined, repository, 'sessions-filters')).toEqual({});
	const expected = { TUI_SESSIONS_FIXTURE_PREFERENCES_DIR: directory };
	expect(sessionsPreferencesEnvironment(directory, repository, 'sessions-filters')).toEqual(
		expected,
	);
	writeFileSync(join(directory, 'agent-sessions-ui.sqlite'), 'retained first invocation');
	expect(sessionsPreferencesEnvironment(directory, repository, 'sessions-filters')).toEqual(
		expected,
	);
});

test('invalid preference nominations fail before driver creation', () => {
	const repository = temporaryRoot();
	const directory = join(repository, 'tmp', 'run', 'preferences');
	mkdirSync(directory, { recursive: true, mode: 0o700 });
	const linked = join(repository, 'tmp', 'linked');
	symlinkSync(directory, linked);
	const file = join(repository, 'tmp', 'file');
	writeFileSync(file, 'not a directory');
	for (const path of ['relative', temporaryRoot(), linked, file, join(directory, 'missing')])
		expect(() => sessionsPreferencesEnvironment(path, repository, 'sessions-filters')).toThrow();
	for (const surface of ['sessions', 'quota', 'normal', 'showcase'])
		expect(() => sessionsPreferencesEnvironment(directory, repository, surface)).toThrow();
	for (const mode of [0o755, 0o770, 0o707]) {
		chmodSync(directory, mode);
		expect(() =>
			sessionsPreferencesEnvironment(directory, repository, 'sessions-filters'),
		).toThrow();
	}
});
