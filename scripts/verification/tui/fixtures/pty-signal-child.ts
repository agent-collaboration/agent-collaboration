/** Real-process caller for the PTY signal ownership control; never a product observation. */
import {
	PtyGridCommandAborted,
	withPtyGridSignalCancellation,
} from '../terminal/pty-grid-cancellation.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';

const [driverPath, evidence] = process.argv.slice(2);
if (driverPath === undefined || evidence === undefined)
	throw new Error('driver and evidence required');
await withPtyGridSignalCancellation(async () => {
	const driver = await PtyGridDriver.start({
		repository: evidence,
		driver: driverPath,
		surface: 'sessions-filters',
		keys: 'legacy',
		evidence,
	});
	try {
		await driver.send(Buffer.from('x'));
		await driver.waitForDispatchFrame(1);
		process.stdout.write('SIGNAL_READY\n');
		await driver.waitForDispatchFrame(2);
	} finally {
		await driver.stop();
	}
}).catch((error: unknown) => {
	if (error instanceof PtyGridCommandAborted) {
		process.stderr.write(error.message + '\n');
		process.exitCode = error.signal === 'SIGINT' ? 130 : 143;
	} else {
		process.stderr.write(String(error) + '\n');
		process.exitCode = 1;
	}
});
