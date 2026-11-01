import { mkdtempSync, realpathSync, renameSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import {
	checkLocalNewOutcome,
	readSourceQualifiedOutcome,
	checkSourceQualifiedOutcome,
	parsePickerOutcomeReadback,
} from './source-qualified-outcome.ts';
import type { NewSessionRequest } from './source-qualified-outcome.ts';
const request = {
	client: 'claudeCode',
	machine: 'localCodex',
	directory: '/fixture/project',
	runMode: 'backgroundThenAttach',
} satisfies NewSessionRequest;
const outcome = { kind: 'new', request };
test('local and default New require the one returned request without an invented preparer count', () => {
	expect(() => checkLocalNewOutcome(outcome, request)).not.toThrow();
	expect(() =>
		checkLocalNewOutcome(
			{ kind: 'new', request: { ...request, machine: 'defaultHosted' } },
			{ ...request, machine: 'defaultHosted' },
		),
	).not.toThrow();
	for (const value of [
		undefined,
		[],
		[outcome, outcome],
		{ ...outcome, callCount: 1 },
		{ kind: 'cancelled' },
		{ kind: 'new', request: { ...request, client: 'codex' } },
		{ kind: 'new', request: { ...request, runMode: 'foreground' } },
		{ kind: 'new', request: { ...request, machine: 'defaultHosted' } },
		{ kind: 'new', request: { ...request, directory: '/other' } },
	])
		expect(() => checkLocalNewOutcome(value, request)).toThrow();
	expect(() =>
		checkLocalNewOutcome(
			{ kind: 'new', request: { ...request, machine: 'configured:fixture' } },
			{ ...request, machine: 'configured:fixture' },
		),
	).toThrow('configured');
});
test('one complete JSON outcome rejects missing and duplicate outcomes', () => {
	expect(parsePickerOutcomeReadback(Buffer.from(JSON.stringify(outcome)))).toEqual(outcome);
	for (const value of [
		'',
		JSON.stringify([outcome, outcome]),
		JSON.stringify(outcome) + '\n' + JSON.stringify(outcome),
		'{',
	])
		expect(() => parsePickerOutcomeReadback(Buffer.from(value))).toThrow();
});
const target = {
	endpoint: { serviceId: '00000000-0000-4000-8000-000000000001', endpointId: 'codex-local' },
	sessionId: 'same-opaque-id',
};
const resume = { kind: 'resume', identity: { kind: 'hostedCodex', target } };
test('U4 compares origin, service and endpoint as well as the opaque same-ID target', () => {
	expect(() => checkSourceQualifiedOutcome(resume, resume)).not.toThrow();
	for (const identity of [
		{ kind: 'localCodex', sessionId: target.sessionId },
		{ kind: 'nativeClaude', target },
		{ kind: 'hostedProvider', target },
		{
			kind: 'hostedCodex',
			target: { ...target, endpoint: { ...target.endpoint, endpointId: 'cursor-local' } },
		},
		{
			kind: 'hostedCodex',
			target: {
				...target,
				endpoint: { ...target.endpoint, serviceId: '00000000-0000-4000-8000-000000000002' },
			},
		},
		{ kind: 'hostedCodex' },
	])
		expect(() => checkSourceQualifiedOutcome({ kind: 'resume', identity }, resume)).toThrow();
	expect(() => checkSourceQualifiedOutcome({ ...resume, kind: 'fork' }, resume)).toThrow();
	expect(() => checkSourceQualifiedOutcome({ ...resume, title: 'extra' }, resume)).toThrow();
});

test('owned outcome file rejects missing and duplicate results without a terminal-marker fallback', () => {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'picker-outcome-')));
	const path = join(root, 'picker-outcome.json');
	try {
		expect(() => readSourceQualifiedOutcome(path, outcome)).toThrow();
		writeFileSync(path, JSON.stringify(outcome));
		expect(readSourceQualifiedOutcome(path, outcome)).toEqual(outcome);
		writeFileSync(path, JSON.stringify(outcome) + '\n' + JSON.stringify(outcome));
		expect(() => readSourceQualifiedOutcome(path, outcome)).toThrow();
		writeFileSync(path, JSON.stringify(outcome));
		renameSync(path, path + '.target');
		symlinkSync(path + '.target', path);
		expect(() => readSourceQualifiedOutcome(path, outcome)).toThrow('owned fixture');
	} finally {
		rmSync(root, { recursive: true, force: true });
	}
});

test('correct fork, cancellation and default foreground Codex remain valid alternatives', () => {
	const fork = { ...resume, kind: 'fork' };
	expect(() => checkSourceQualifiedOutcome(fork, fork)).not.toThrow();
	expect(() =>
		checkSourceQualifiedOutcome({ kind: 'cancelled' }, { kind: 'cancelled' }),
	).not.toThrow();
	const defaultRequest = {
		client: 'codex',
		machine: 'defaultHosted',
		directory: '/fixture/project',
		runMode: 'foreground',
	} satisfies NewSessionRequest;
	expect(() =>
		checkLocalNewOutcome({ kind: 'new', request: defaultRequest }, defaultRequest),
	).not.toThrow();
});
