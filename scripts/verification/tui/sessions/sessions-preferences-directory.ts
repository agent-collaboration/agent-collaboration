/** Explicit fixture persistence stays in a private per-run directory under the observer lane tmp. */
import { lstatSync, realpathSync } from 'node:fs';
import { isAbsolute, join, relative, sep } from 'node:path';

export const SESSIONS_PREFERENCES_DIRECTORY = 'TUI_SESSIONS_FIXTURE_PREFERENCES_DIR';

function rejectPreferenceDirectory(): never {
	throw new Error('picker preferences require an owned private canonical directory under lane tmp');
}

export function sessionsPreferencesEnvironment(
	directory: string | undefined,
	repository: string,
	surface: string,
): Readonly<Record<string, string>> {
	if (directory === undefined) return {};
	if (surface !== 'sessions-filters' || !isAbsolute(directory)) return rejectPreferenceDirectory();
	try {
		const temporary = realpathSync(join(repository, 'tmp'));
		const within = relative(temporary, directory);
		const metadata = lstatSync(directory);
		if (
			within === '' ||
			within === '..' ||
			within.startsWith('..' + sep) ||
			isAbsolute(within) ||
			realpathSync(directory) !== directory ||
			!metadata.isDirectory() ||
			metadata.isSymbolicLink() ||
			metadata.uid !== process.getuid?.() ||
			(metadata.mode & 0o077) !== 0
		)
			return rejectPreferenceDirectory();
	} catch {
		return rejectPreferenceDirectory();
	}
	return { [SESSIONS_PREFERENCES_DIRECTORY]: directory };
}
