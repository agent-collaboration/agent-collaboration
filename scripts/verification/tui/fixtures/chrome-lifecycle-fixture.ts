/** Controlled capture process: the real browser owner must settle before a final error exits. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { reportCliFailure } from '../cli/cli-arguments.ts';
import { ChromeDevtools } from '../terminal/chrome-devtools.ts';
import { renderOriginal } from '../terminal/terminal-original.ts';

async function forceFinalError(): Promise<void> {
	const [browser, evidence, mode] = process.argv.slice(2);
	if (browser === undefined || evidence === undefined)
		throw new Error('browser and evidence required');
	if (mode === 'close-error') {
		// Denial is confined to this controlled process; the actual listener must still close.
		const originalKill = process.kill.bind(process);
		process.kill = (pid, signal): true => {
			if (pid < 0) throw Object.assign(new Error('forced group kill EPERM'), { code: 'EPERM' });
			return originalKill(pid, signal);
		};
		try {
			await renderOriginal({
				chrome: browser,
				output: evidence,
				bytes: Buffer.alloc(0),
				columns: 120,
				rows: 36,
				checkpoint: 'controlled',
				sourceHead: 'a'.repeat(40),
				binarySha256: 'b'.repeat(64),
			});
		} finally {
			process.kill = originalKill;
		}
		return;
	}
	const chrome = new ChromeDevtools(browser, evidence);
	try {
		const receipt = await chrome.send('Fixture.ready');
		writeFileSync(join(evidence, 'browser-ready.json'), JSON.stringify(receipt));
		process.send?.(receipt);
	} finally {
		await chrome.close();
	}
	throw new Error('forced final observation error');
}

forceFinalError().catch(reportCliFailure);
