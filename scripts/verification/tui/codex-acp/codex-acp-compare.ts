/**
 * `node codex-acp-compare.ts --reference <run dir> --candidate <run dir> [--verdict <file>]`: the O5
 * two-run comparison. Reads the evidence of one real-Codex reference run and one Router candidate
 * run (each written by a `compare` journey run), applies k1-k4 and the usage rule to each, then k5
 * across them, and writes a JSON verdict. The reference's own result comes first: a rule real Codex
 * breaks is reported as `rule-contradicted-by-reference` and is never relaxed. Exits 0 only with
 * zero violations of every kind.
 *
 * The verdict never says the O5 qualification is complete: `not_covered` names what these runs
 * cannot show.
 */
import { existsSync, writeFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { parseArgs } from 'node:util';

import {
	compareRuns,
	loadCompareRun,
} from '../../../../verification/observations/providers/acp/codex-compare-evaluation.ts';
import type { CompareVerdict } from '../../../../verification/observations/providers/acp/codex-compare-evaluation.ts';

export const COMPARE_VERDICT_FILE = 'o5-compare-verdict.json';

/** What a `compare` journey run does not cover; the verdict carries it so a pass is not read as more. */
export const COMPARISON_NOT_COVERED = [
	'two CLI windows at once: not built (the design states no cross-window rule); the driver has one PTY and the evaluation refuses a two-connection recording',
	'/resume, refused commands and the service-tier pick: built and unit-tested, not run (their own journeys, judged by codex-acp-moved.ts); live run in D3',
	'approvals and their agent-side outcome: built and unit-tested, not run (their own journey, judged by codex-acp-moved.ts); live run in D4',
] as const;

export interface CompareReport extends CompareVerdict {
	readonly reference_run: string;
	readonly candidate_run: string;
	readonly s4_complete: false;
	readonly not_covered: readonly string[];
}

function requireAbsolute(flag: string, value: string | undefined): string {
	if (value === undefined || !isAbsolute(value))
		throw new Error(`${flag} <absolute directory> is required`);
	return value;
}

/** Runs the comparison and writes the verdict file; returns the process exit code. */
export function runCompareCommand(argv: readonly string[]): {
	exitCode: number;
	verdictPath: string;
} {
	const { values } = parseArgs({
		args: [...argv],
		options: {
			reference: { type: 'string' },
			candidate: { type: 'string' },
			verdict: { type: 'string' },
		},
		strict: true,
	});
	const referenceDirectory = requireAbsolute('--reference', values.reference);
	const candidateDirectory = requireAbsolute('--candidate', values.candidate);
	const verdictPath = values.verdict ?? join(candidateDirectory, COMPARE_VERDICT_FILE);
	if (!isAbsolute(verdictPath)) throw new Error('--verdict must be an absolute path');
	if (existsSync(verdictPath)) throw new Error('verdict file already exists; choose a fresh path');
	const verdict = compareRuns(
		loadCompareRun(referenceDirectory),
		loadCompareRun(candidateDirectory),
	);
	const report: CompareReport = {
		// The reference's own result leads the file.
		status: verdict.status,
		reference: verdict.reference,
		candidate: verdict.candidate,
		crossRun: verdict.crossRun,
		reference_run: referenceDirectory,
		candidate_run: candidateDirectory,
		s4_complete: false,
		not_covered: COMPARISON_NOT_COVERED,
	};
	writeFileSync(verdictPath, JSON.stringify(report, null, 2), { mode: 0o600, flag: 'wx' });
	return { exitCode: verdict.status === 'pass' ? 0 : 1, verdictPath };
}

if (import.meta.main) {
	const { exitCode, verdictPath } = runCompareCommand(process.argv.slice(2));
	process.stdout.write(`${verdictPath}\n`);
	process.exitCode = exitCode;
}
