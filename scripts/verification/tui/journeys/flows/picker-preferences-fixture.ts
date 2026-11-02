/** Malformed R19 input is confined to the aggregate case's existing private fixture store. */
import { lstatSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

import { z } from 'zod';

import { fileDigest } from '../../evidence/source-binding.ts';
import { sessionsPreferencesEnvironment } from '../../sessions/sessions-preferences-directory.ts';

function storePath(repository: string, directory: string): string {
	sessionsPreferencesEnvironment(directory, repository, 'sessions-filters');
	return join(directory, 'agent-sessions-ui.sqlite');
}
function existingStore(repository: string, directory: string): string {
	const path = storePath(repository, directory),
		metadata = lstatSync(path);
	if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.uid !== process.getuid?.())
		throw new Error('R19 fixture preference store is not the owned regular file');
	return path;
}
export function readPickerRememberedGrouping(repository: string, directory: string): string | null {
	const database = new DatabaseSync(existingStore(repository, directory), { readOnly: true });
	try {
		const row = database
			.prepare("SELECT value FROM ui_preferences WHERE screen='sessions' AND setting='group-by'")
			.get();
		return row === undefined ? null : z.object({ value: z.string() }).parse(row).value;
	} finally {
		database.close();
	}
}
export function seedUnknownPickerGrouping(
	repository: string,
	directory: string,
): { readonly before: string; readonly after: string } {
	const before = readPickerRememberedGrouping(repository, directory);
	if (before !== 'provider')
		throw new Error('unknown preference control requires the real previously saved Provider value');
	const database = new DatabaseSync(existingStore(repository, directory));
	try {
		const result = database
			.prepare("UPDATE ui_preferences SET value=? WHERE screen='sessions' AND setting='group-by'")
			.run('fixture-unknown-group');
		if (Number(result.changes) !== 1)
			throw new Error('unknown preference seed did not change exactly one existing row');
	} finally {
		database.close();
	}
	const after = readPickerRememberedGrouping(repository, directory);
	if (after !== 'fixture-unknown-group')
		throw new Error('unknown preference seed was not persisted');
	return { before, after };
}
export function createUnreadablePickerPreferences(
	repository: string,
	directory: string,
): { readonly path: string; readonly sha256: string } {
	const path = storePath(repository, directory);
	writeFileSync(path, 'R19 unreadable preference fixture\n', { flag: 'wx', mode: 0o600 });
	return { path, sha256: fileDigest(path) };
}
