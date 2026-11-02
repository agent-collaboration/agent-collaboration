/** Keep only the admitted wrapper alive until its owning driver stops it. */
import { writeFileSync, writeSync } from 'node:fs';
import { join } from 'node:path';

export async function holdFixtureHandshake(output: string): Promise<void> {
	await new Promise<void>((resolve) => {
		const stop = (): void => {
			process.off('SIGTERM', stop);
			process.off('SIGINT', stop);
			process.stdin.off('end', stop);
			process.stdin.pause();
			resolve();
		};
		process.once('SIGTERM', stop);
		process.once('SIGINT', stop);
		process.stdin.once('end', stop);
		// Only this no-example qualification mode holds the inherited input open.
		process.stdin.resume();
		writeSync(1, 'TUI_SHOWCASE_WRAPPER_HANDSHAKE_READY\n');
	});
	writeFileSync(join(output, 'wrapper-handshake.exit'), '0');
}
