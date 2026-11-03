/** Real SQLite engine controls for isolated malformed preference input, never product proof. */
import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

import { expect, test } from 'vitest';

import { temporaryRoot } from '../../fixtures/receipt-fixtures.ts';
import {
	readPickerRememberedGrouping,
	seedUnknownPickerGrouping,
	createUnreadablePickerPreferences,
} from './picker-preferences-fixture.ts';

test('unknown facet seeding changes exactly the existing sessions value and preserves unrelated records', () => {
	const repository = temporaryRoot(),
		preferences = join(repository, 'tmp', 'case', 'preferences');
	mkdirSync(preferences, { recursive: true, mode: 0o700 });
	const database = new DatabaseSync(join(preferences, 'agent-sessions-ui.sqlite'));
	database.exec(
		'CREATE TABLE ui_preferences(preference_id TEXT PRIMARY KEY NOT NULL,screen TEXT NOT NULL,setting TEXT NOT NULL,value TEXT NOT NULL,UNIQUE(screen,setting));',
	);
	database
		.prepare('INSERT INTO ui_preferences VALUES(?,?,?,?)')
		.run('018f47d2-24d5-7a68-b9ec-6f759c39458f', 'sessions', 'group-by', 'provider');
	database
		.prepare('INSERT INTO ui_preferences VALUES(?,?,?,?)')
		.run('018f47d2-24d5-7a68-b9ec-6f759c394590', 'quota', 'group-by', 'status');
	database.close();
	expect(readPickerRememberedGrouping(repository, preferences)).toBe('provider');
	expect(seedUnknownPickerGrouping(repository, preferences)).toEqual({
		before: 'provider',
		after: 'fixture-unknown-group',
	});
	const reopened = new DatabaseSync(join(preferences, 'agent-sessions-ui.sqlite'), {
		readOnly: true,
	});
	try {
		expect(reopened.prepare('SELECT * FROM ui_preferences ORDER BY screen').all()).toEqual([
			{
				preference_id: '018f47d2-24d5-7a68-b9ec-6f759c394590',
				screen: 'quota',
				setting: 'group-by',
				value: 'status',
			},
			{
				preference_id: '018f47d2-24d5-7a68-b9ec-6f759c39458f',
				screen: 'sessions',
				setting: 'group-by',
				value: 'fixture-unknown-group',
			},
		]);
	} finally {
		reopened.close();
	}
});

test('unreadable input is fresh, exact, and never overwrites an existing file or escapes lane tmp', () => {
	const repository = temporaryRoot(),
		preferences = join(repository, 'tmp', 'case', 'preferences');
	mkdirSync(preferences, { recursive: true, mode: 0o700 });
	const created = createUnreadablePickerPreferences(repository, preferences);
	expect(readFileSync(created.path, 'utf8')).toBe('R19 unreadable preference fixture\n');
	expect(() => createUnreadablePickerPreferences(repository, preferences)).toThrow();
	expect(() => createUnreadablePickerPreferences(repository, temporaryRoot())).toThrow('lane tmp');
});
