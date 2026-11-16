import { mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';
import { z } from 'zod';

import {
	PROVIDER_USAGE_FILE,
	RUN_RECORD_FILE,
} from '../../../../verification/observations/providers/acp/codex-compare-evaluation.ts';
import type { CompareRunRecord } from '../../../../verification/observations/providers/acp/codex-compare-evaluation.ts';
import {
	SCRIPTED,
	scriptRun,
} from '../../../../verification/observations/providers/acp/codex-compare-fixtures.ts';
import type { ScriptRunOptions } from '../../../../verification/observations/providers/acp/codex-compare-fixtures.ts';
import { COMPARE_VERDICT_FILE, runCompareCommand } from './codex-acp-compare.ts';

const verdictSchema = z.looseObject({
	status: z.string(),
	reference: z.array(z.looseObject({ rule: z.string() })),
	candidate: z.array(z.looseObject({ rule: z.string() })),
	crossRun: z.array(z.looseObject({ rule: z.string() })),
	s4_complete: z.boolean(),
	not_covered: z.array(z.string()),
});

const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

interface RunDirectoryInput {
	readonly kind: CompareRunRecord['run_kind'];
	readonly script: ScriptRunOptions;
	readonly usage?: readonly { turn_ordinal: number; used: number; size: number }[];
}

function runDirectory(root: string, name: string, input: RunDirectoryInput): string {
	const directory = join(root, name);
	mkdirSync(directory);
	writeFileSync(
		join(directory, 'wire.jsonl'),
		scriptRun(input.script)
			.build()
			.map((frame) => JSON.stringify(frame))
			.join('\n'),
	);
	const record: CompareRunRecord = {
		run_kind: input.kind,
		wire_recording: 'wire.jsonl',
		scripted_settings: SCRIPTED,
		agent_approval_outcomes: null,
	};
	writeFileSync(join(directory, RUN_RECORD_FILE), JSON.stringify(record));
	if (input.usage !== undefined)
		writeFileSync(join(directory, PROVIDER_USAGE_FILE), JSON.stringify(input.usage));
	return directory;
}

const USAGE_1M = [{ turn_ordinal: 0, used: 4000, size: 1_000_000 }];

function compare(
	candidate: Partial<RunDirectoryInput> & { readonly script: ScriptRunOptions },
	reference: ScriptRunOptions = { turnId: 'R1', window: 258_000 },
): { exitCode: number; verdict: z.infer<typeof verdictSchema>; root: string } {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'o5-cmp-')));
	roots.push(root);
	const referenceDirectory = runDirectory(root, 'reference', {
		kind: 'codex-reference',
		script: { ...reference, referenceLaunch: true },
	});
	const candidateDirectory = runDirectory(root, 'candidate', {
		kind: 'router-candidate',
		usage: USAGE_1M,
		...candidate,
	});
	const { exitCode, verdictPath } = runCompareCommand([
		'--reference',
		referenceDirectory,
		'--candidate',
		candidateDirectory,
	]);
	expect(verdictPath).toBe(join(candidateDirectory, COMPARE_VERDICT_FILE));
	return {
		exitCode,
		verdict: verdictSchema.parse(JSON.parse(readFileSync(verdictPath, 'utf8'))),
		root,
	};
}

const VALID = { turnId: 'C1', window: 1_000_000 } as const;

test('matching runs exit 0, and the verdict still says S4 is not complete', () => {
	const { exitCode, verdict } = compare({ script: VALID });
	expect(exitCode).toBe(0);
	expect(verdict).toMatchObject({ status: 'pass', reference: [], candidate: [], crossRun: [] });
	expect(verdict.s4_complete).toBe(false);
	expect(verdict.not_covered).toEqual(
		expect.arrayContaining([expect.stringContaining('two CLI windows')]),
	);
});

test.each([
	['a dropped steer receipt', { ...VALID, dropSteerReceipt: true }, 'k3'],
	['a duplicated receipt', { ...VALID, duplicateSteerReceipt: true }, 'k2'],
	['an agent item before the opening user item', { ...VALID, agentBeforeUser: true }, 'k2'],
	['a 770K window where the provider reported 1M', { ...VALID, window: 770_000 }, 'usage'],
] as const)('%s exits 1 with the candidate violation', (_label, script, rule) => {
	const { exitCode, verdict } = compare({ script });
	expect(exitCode).toBe(1);
	expect(verdict.status).toBe('fail');
	expect(verdict.candidate).toEqual(expect.arrayContaining([expect.objectContaining({ rule })]));
});

test('a lost model change exits 1 through the correspondence rule', () => {
	const { exitCode, verdict } = compare({
		script: { ...VALID, settings: ['old-model', 'low', null] },
	});
	expect(exitCode).toBe(1);
	expect(verdict.crossRun).toEqual(
		expect.arrayContaining([expect.objectContaining({ rule: 'k5' })]),
	);
});

test('a candidate without the provider usage report exits 1', () => {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'o5-cmp-')));
	roots.push(root);
	const reference = runDirectory(root, 'reference', {
		kind: 'codex-reference',
		script: { turnId: 'R1', window: 258_000, referenceLaunch: true },
	});
	const candidate = runDirectory(root, 'candidate', { kind: 'router-candidate', script: VALID });
	const { exitCode } = runCompareCommand(['--reference', reference, '--candidate', candidate]);
	expect(exitCode).toBe(1);
	const verdict = verdictSchema.parse(
		JSON.parse(readFileSync(join(candidate, COMPARE_VERDICT_FILE), 'utf8')),
	);
	expect(verdict.candidate.map((violation) => violation.rule)).toEqual(['usage']);
});

test('a rule broken by real Codex is reported first as contradicted by the reference', () => {
	const { exitCode, verdict } = compare(
		{ script: VALID },
		{ turnId: 'R1', window: 258_000, dropSteerReceipt: true },
	);
	expect(exitCode).toBe(1);
	expect(verdict.status).toBe('rule-contradicted-by-reference');
	expect(Object.keys(verdict)[0]).toBe('status');
	expect(verdict.reference).toEqual(
		expect.arrayContaining([expect.objectContaining({ rule: 'k3' })]),
	);
});

test('relative paths and an existing verdict file are refused', () => {
	const { root } = compare({ script: VALID });
	expect(() =>
		runCompareCommand(['--reference', 'reference', '--candidate', join(root, 'candidate')]),
	).toThrow('absolute');
	expect(() =>
		runCompareCommand([
			'--reference',
			join(root, 'reference'),
			'--candidate',
			join(root, 'candidate'),
		]),
	).toThrow('already exists');
	expect(() => runCompareCommand(['--reference', join(root, 'reference')])).toThrow('--candidate');
});
