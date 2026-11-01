/** Owned, opt-in receipt directory passed only to the sessions fixture entry. */
import { lstatSync, readdirSync, realpathSync } from 'node:fs';
import { dirname, isAbsolute } from 'node:path';

export const SESSIONS_RECEIPT_DIRECTORY = 'TUI_SESSIONS_FIXTURE_RECEIPT_DIR';

function rejectReceiptDirectory(): never {
	throw new Error(
		'sessions receipt directory must be an empty owned directory directly inside output',
	);
}

export function sessionsReceiptEnvironment(
	directory: string | undefined,
	output: string,
): Readonly<Record<string, string>> {
	if (directory === undefined) return {};
	if (!isAbsolute(directory) || !isAbsolute(output)) return rejectReceiptDirectory();
	try {
		const metadata = lstatSync(directory);
		const outputMetadata = lstatSync(output);
		if (
			!metadata.isDirectory() ||
			metadata.isSymbolicLink() ||
			!outputMetadata.isDirectory() ||
			outputMetadata.isSymbolicLink() ||
			metadata.uid !== process.getuid?.() ||
			realpathSync(dirname(directory)) !== realpathSync(output) ||
			readdirSync(directory).length !== 0
		)
			return rejectReceiptDirectory();
	} catch {
		return rejectReceiptDirectory();
	}
	return { [SESSIONS_RECEIPT_DIRECTORY]: directory };
}
