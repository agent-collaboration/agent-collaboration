import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { requestStopFile } from './driver-lifecycle.ts';
import type { DriverChild } from './driver-lifecycle.ts';
function waitBounded(completion: Promise<number>, seconds: number): Promise<number | null> {
	return new Promise((resolve, reject) => {
		const timer = setTimeout(() => resolve(null), seconds * 1000);
		completion.then(
			(exit) => {
				clearTimeout(timer);
				resolve(exit);
			},
			(error: unknown) => {
				clearTimeout(timer);
				reject(error);
			},
		);
	});
}
export async function waitPublicDriver(
	child: DriverChild,
	output: string,
	budget: { readonly workSeconds: number; readonly cleanupSeconds: number } = {
		workSeconds: 660,
		cleanupSeconds: 90,
	},
): Promise<void> {
	let stopError: string | null = null;
	let result = await waitBounded(child.completion, budget.workSeconds);
	const timedOut = result === null;
	if (timedOut) {
		try {
			requestStopFile(join(output, 'cua-sessions/cua-stop'));
		} catch (error) {
			stopError = String(error);
		}
		result = await waitBounded(child.completion, budget.cleanupSeconds);
	}
	writeFileSync(
		join(output, 'driver-exit.json'),
		JSON.stringify({
			pid: child.process.pid,
			exit: result,
			deadline_reached: timedOut,
			driver_settled: result !== null,
			stop_error: stopError,
		}),
	);
	if (result === null) child.process.unref();
	if (timedOut || result !== 0)
		throw new Error('public CUA failed or cleanup unsettled; retain evidence, do not replay');
}
