/**
 * `node codex-acp-approval-compare.ts --reference <run dir> --candidate <run dir> [--verdict <file>]`:
 * the O5 approval comparison. Reads one real-Codex reference run and one Router candidate run (each
 * with two windows, written by an `approval` journey run), applies a1-a3, and writes a JSON
 * verdict. The reference's own result comes first: a rule real Codex breaks is reported as
 * `rule-contradicted-by-reference` and never relaxed. Exits 0 only with zero violations.
 * Built and unit-tested only; D4 runs it live.
 */
import { existsSync, writeFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { parseArgs } from 'node:util';

import {
	ACP_OPTIONS_WITHOUT_CODEX_EQUIVALENT,
	CODEX_DECISIONS_WITHOUT_ACP_OPTION,
	compareApprovalRuns,
	loadApprovalRun,
} from '../../../../verification/observations/providers/acp/codex-approval-compare.ts';
import type { ApprovalCompareVerdict } from '../../../../verification/observations/providers/acp/codex-approval-compare.ts';

export const APPROVAL_VERDICT_FILE = 'o5-approval-verdict.json';

export interface ApprovalCompareReport extends ApprovalCompareVerdict {
	readonly reference_run: string;
	readonly candidate_run: string;
	readonly live_run: 'not run: built and unit-tested only';
	// What N7's table leaves without an equivalent, so a pass is not read as covering it.
	readonly acp_options_without_codex_equivalent: readonly string[];
	readonly codex_decisions_without_acp_option: readonly string[];
}

function requireAbsolute(flag: string, value: string | undefined): string {
	if (value === undefined || !isAbsolute(value))
		throw new Error(`${flag} <absolute directory> is required`);
	return value;
}

/** Runs the comparison and writes the verdict file; returns the process exit code. */
export function runApprovalCompareCommand(argv: readonly string[]): {
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
	const reference = requireAbsolute('--reference', values.reference);
	const candidate = requireAbsolute('--candidate', values.candidate);
	const verdictPath = values.verdict ?? join(candidate, APPROVAL_VERDICT_FILE);
	if (!isAbsolute(verdictPath)) throw new Error('--verdict must be an absolute path');
	if (existsSync(verdictPath)) throw new Error('verdict file already exists; choose a fresh path');
	const verdict = compareApprovalRuns(loadApprovalRun(reference), loadApprovalRun(candidate));
	const report: ApprovalCompareReport = {
		status: verdict.status,
		reference: verdict.reference,
		candidate: verdict.candidate,
		crossRun: verdict.crossRun,
		reference_run: reference,
		candidate_run: candidate,
		live_run: 'not run: built and unit-tested only',
		acp_options_without_codex_equivalent: ACP_OPTIONS_WITHOUT_CODEX_EQUIVALENT,
		codex_decisions_without_acp_option: CODEX_DECISIONS_WITHOUT_ACP_OPTION,
	};
	writeFileSync(verdictPath, JSON.stringify(report, null, 2), { mode: 0o600, flag: 'wx' });
	return { exitCode: verdict.status === 'pass' ? 0 : 1, verdictPath };
}

if (import.meta.main) {
	const { exitCode, verdictPath } = runApprovalCompareCommand(process.argv.slice(2));
	process.stdout.write(`${verdictPath}\n`);
	process.exitCode = exitCode;
}
