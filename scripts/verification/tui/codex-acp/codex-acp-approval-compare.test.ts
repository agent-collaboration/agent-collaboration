import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { APPROVAL_RUN_FILE } from '../../../../verification/observations/providers/acp/codex-approval-compare.ts';
import { approvalPairRun } from '../../../../verification/observations/providers/acp/codex-approval-fixtures.ts';
import type { ApprovalPairOptions } from '../../../../verification/observations/providers/acp/codex-approval-fixtures.ts';
import {
	APPROVAL_OUTCOME_FILE,
	approvalOutcomeFile,
} from '../../../../verification/observations/providers/acp/codex-approval-outcome.ts';
import { APPROVAL_VERDICT_FILE, runApprovalCompareCommand } from './codex-acp-approval-compare.ts';

const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

const option = (
	effect: string,
	scope: string,
): { optionId: string; effect: string; scope: string; persistentTarget: null } => ({
	optionId: `${effect}-${scope}`,
	effect,
	scope,
	persistentTarget: null,
});
const record = (
	state: string,
): { requestId: string; state: string; options: ReturnType<typeof option>[] } => ({
	requestId: 'r1',
	state,
	options: [option('allow', 'once'), option('decline', 'once')],
});

function runDirectory(
	root: string,
	name: string,
	kind: 'codex-reference' | 'router-candidate',
	wire: ApprovalPairOptions,
): string {
	const directory = join(root, name);
	mkdirSync(directory);
	writeFileSync(
		join(directory, 'wire.jsonl'),
		approvalPairRun(wire)
			.map((frame) => JSON.stringify(frame))
			.join('\n'),
	);
	writeFileSync(
		join(directory, APPROVAL_RUN_FILE),
		JSON.stringify({
			run_kind: kind,
			wire_recording: 'wire.jsonl',
			window_a: 0,
			window_b: 1,
			action_kind: 'command',
			cli_choice: 'accept',
		}),
	);
	if (kind === 'router-candidate')
		writeFileSync(
			join(directory, APPROVAL_OUTCOME_FILE),
			JSON.stringify(
				approvalOutcomeFile({
					case: 'allow-once',
					cliDecision: 'accept',
					readings: {
						pending: {
							...record('pendingClientDecision'),
						},
						final: record('decided'),
						filePresent: true,
					},
				}),
			),
		);
	return directory;
}

function compare(candidateWire: ApprovalPairOptions): { exitCode: number; verdict: unknown } {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'apr-cli-')));
	roots.push(root);
	const reference = runDirectory(root, 'ref', 'codex-reference', {});
	const candidate = runDirectory(root, 'cand', 'router-candidate', candidateWire);
	const { exitCode, verdictPath } = runApprovalCompareCommand([
		'--reference',
		reference,
		'--candidate',
		candidate,
	]);
	expect(verdictPath).toBe(join(candidate, APPROVAL_VERDICT_FILE));
	return { exitCode, verdict: JSON.parse(readFileSync(verdictPath, 'utf8')) as unknown };
}

test('matching runs exit 0, and the verdict names what N7 leaves without an equivalent', () => {
	const { exitCode, verdict } = compare({});
	expect(exitCode).toBe(0);
	expect(verdict).toMatchObject({
		status: 'pass',
		live_run: 'not run: built and unit-tested only',
		codex_decisions_without_acp_option: ['cancel'],
	});
	expect(verdict).toHaveProperty('acp_options_without_codex_equivalent');
});

test.each([
	['a different request method', { method: 'item/fileChange/requestApproval' }],
	['a missing choice', { choices: ['accept', 'decline', 'cancel'] }],
	['a prompt only in text', { textOnly: true }],
	['both windows prompted', { bPrompted: true }],
	['a result never shown in the second window', { bMissesResult: true }],
	['a CLI answer that is not the scripted one', { decision: 'decline' }],
] as const)('%s exits 1', (_name, wire) => {
	const { exitCode, verdict } = compare(wire);
	expect(exitCode).toBe(1);
	expect(verdict).toMatchObject({ status: 'fail' });
});

test('a verdict is never overwritten', () => {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'apr-cli-')));
	roots.push(root);
	const reference = runDirectory(root, 'ref', 'codex-reference', {});
	const candidate = runDirectory(root, 'cand', 'router-candidate', {});
	runApprovalCompareCommand(['--reference', reference, '--candidate', candidate]);
	expect(() =>
		runApprovalCompareCommand(['--reference', reference, '--candidate', candidate]),
	).toThrow('already exists');
});
