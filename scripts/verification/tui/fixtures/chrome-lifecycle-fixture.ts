/** Controlled capture process: the real browser owner must settle before a final error exits. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { reportCliFailure } from '../cli/cli-arguments.ts';
import { ChromeDevtools } from '../terminal/chrome-devtools.ts';

async function forceFinalError(): Promise<void> {
	const [browser, evidence] = process.argv.slice(2);
	if (browser === undefined || evidence === undefined)
		throw new Error('browser and evidence required');
	const chrome = new ChromeDevtools(browser, evidence);
	try {
		const receipt = await chrome.send('Fixture.ready');
		writeFileSync(join(evidence, 'browser-ready.json'), JSON.stringify(receipt));
	} finally {
		await chrome.close();
	}
	throw new Error('forced final observation error');
}

forceFinalError().catch(reportCliFailure);
