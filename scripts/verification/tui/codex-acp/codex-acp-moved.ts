/**
 * `node codex-acp-moved.ts --run <run dir> [--verdict <file>]`: judges one moved-journey run
 * (`/resume`, the service-tier pick or approval) from the wire recording and, for approval, the
 * agent-side outcome file the run kept. Writes a JSON verdict and exits 0 only with zero
 * violations. Built and unit-tested only; the live runs are D3's (resume, tier) and D4's
 * (approval).
 */
import { existsSync, writeFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { parseArgs } from 'node:util';

import {
	evaluateMovedRun,
	loadMovedRun,
} from '../../../../verification/observations/providers/acp/codex-moved-evaluation.ts';
import type { MovedViolation } from '../../../../verification/observations/providers/acp/codex-moved-evaluation.ts';

export const MOVED_VERDICT_FILE = 'moved-verdict.json';

export interface MovedReport {
	readonly status: 'pass' | 'fail';
	readonly kind: string;
	readonly violations: readonly MovedViolation[];
	readonly run: string;
	readonly live_run: 'not run: built and unit-tested only';
}

/** Runs the evaluation and writes the verdict file; returns the process exit code. */
export function runMovedCommand(argv: readonly string[]): {
	exitCode: number;
	verdictPath: string;
} {
	const { values } = parseArgs({
		args: [...argv],
		options: { run: { type: 'string' }, verdict: { type: 'string' } },
		strict: true,
	});
	if (values.run === undefined || !isAbsolute(values.run))
		throw new Error('--run <absolute directory> is required');
	const verdictPath = values.verdict ?? join(values.run, MOVED_VERDICT_FILE);
	if (!isAbsolute(verdictPath)) throw new Error('--verdict must be an absolute path');
	if (existsSync(verdictPath)) throw new Error('verdict file already exists; choose a fresh path');
	const run = loadMovedRun(values.run);
	const violations = evaluateMovedRun(run);
	const report: MovedReport = {
		status: violations.length === 0 ? 'pass' : 'fail',
		kind: run.record.kind,
		violations,
		run: values.run,
		live_run: 'not run: built and unit-tested only',
	};
	writeFileSync(verdictPath, JSON.stringify(report, null, 2), { mode: 0o600, flag: 'wx' });
	return { exitCode: violations.length === 0 ? 0 : 1, verdictPath };
}

if (import.meta.main) {
	const { exitCode, verdictPath } = runMovedCommand(process.argv.slice(2));
	process.stdout.write(`${verdictPath}\n`);
	process.exitCode = exitCode;
}
