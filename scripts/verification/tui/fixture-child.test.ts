import { createHash } from 'node:crypto';
import {
	chmodSync,
	mkdirSync,
	mkdtempSync,
	realpathSync,
	rmSync,
	symlinkSync,
	writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import {
	childLaunchConfiguration,
	fixtureCommandPath,
	FixtureLaunchRejected,
	readFixtureCommand,
	verifyComponentCompletion,
} from './fixture-child.ts';
import type { FixtureCommand } from './fixture-child.ts';

const roots: string[] = [];
function temporaryDirectory(): string {
	const root = realpathSync(mkdtempSync(join(tmpdir(), 'fixture-child-')));
	roots.push(root);
	return root;
}
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

const COMPONENT_ARGUMENTS = [
	'quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture',
	'--exact',
	'--ignored',
	'--nocapture',
	'--test-threads=1',
];

test('component_pending_receipt_requires_exact_release_and_fresh_paths', () => {
	const output = temporaryDirectory();
	const receipt = join(output, 'receipt');
	const release = join(output, 'release');
	const configuration: FixtureCommand = {
		surface: 'quota-component',
		binary: '/declared/test',
		temporary_directory: output,
		output,
		component_fixture_scenario: 'commit-pending',
		component_fixture_receipt: receipt,
		component_fixture_release: release,
	};
	const launch = childLaunchConfiguration(configuration);
	expect(launch.arguments).toEqual(COMPONENT_ARGUMENTS);
	expect(launch.environment).toEqual({
		HOME: output,
		TERM: 'xterm-256color',
		LANG: 'C.UTF-8',
		TMPDIR: output,
		QUOTA_COMPONENT_FIXTURE: 'commit-pending',
		QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE: receipt,
		QUOTA_COMPONENT_FIXTURE_RELEASE_FILE: release,
	});
	const marker =
		'QUOTA_COMPONENT_FIXTURE_FINISHED scenario=CommitPending released=true external_requests=0 persisted_writes=0\n';
	writeFileSync(receipt, marker);
	expect(() => verifyComponentCompletion(configuration)).toThrow('exact owned release');
	writeFileSync(release, 'wrong\n');
	expect(() => verifyComponentCompletion(configuration)).toThrow('exact owned release');
	writeFileSync(release, 'settle\n');
	verifyComponentCompletion(configuration);
	writeFileSync(receipt, marker.replace('CommitPending', 'Normal'));
	expect(() => verifyComponentCompletion(configuration)).toThrow('receipt mismatch');
	// Both paths now exist, so a second launch from this record must refuse.
	expect(() => childLaunchConfiguration(configuration)).toThrow('must be absent');
});

test('component_receipt_must_be_the_exact_unreleased_marker', () => {
	const output = temporaryDirectory();
	const receipt = join(output, 'fixture-finished');
	const configuration: FixtureCommand = {
		surface: 'quota-component',
		binary: '/declared/test',
		temporary_directory: output,
		output,
		component_fixture_scenario: 'normal',
		component_fixture_receipt: receipt,
	};
	expect(childLaunchConfiguration(configuration).environment).not.toHaveProperty(
		'QUOTA_COMPONENT_FIXTURE_RELEASE_FILE',
	);
	expect(() => verifyComponentCompletion(configuration)).toThrow('completion receipt missing');
	const exact =
		'QUOTA_COMPONENT_FIXTURE_FINISHED scenario=Normal released=false external_requests=0 persisted_writes=0\n';
	for (const forged of [
		exact.trimEnd(),
		exact.replace('external_requests=0', 'external_requests=1'),
		`${exact}extra\n`,
	]) {
		writeFileSync(receipt, forged);
		expect(() => verifyComponentCompletion(configuration)).toThrow('receipt mismatch');
	}
	writeFileSync(receipt, exact);
	verifyComponentCompletion(configuration);
});

test('component_receipt_and_release_must_be_distinct_fresh_paths_inside_output', () => {
	const output = temporaryDirectory();
	const outside = temporaryDirectory();
	const base: FixtureCommand = {
		surface: 'quota-component',
		binary: '/declared/test',
		temporary_directory: output,
		output,
		component_fixture_scenario: 'commit-pending',
		component_fixture_receipt: join(output, 'receipt'),
		component_fixture_release: join(output, 'release'),
	};
	for (const change of [
		{ component_fixture_receipt: join(outside, 'receipt') },
		{ component_fixture_receipt: 'relative-receipt' },
		{ component_fixture_release: join(output, 'receipt') },
	]) {
		expect(() => childLaunchConfiguration({ ...base, ...change })).toThrow(FixtureLaunchRejected);
	}
	const linked = join(output, 'linked-receipt');
	symlinkSync(join(outside, 'target'), linked);
	expect(() => childLaunchConfiguration({ ...base, component_fixture_receipt: linked })).toThrow(
		'must be absent',
	);
});

test('adapter_keeps_existing_surfaces_and_rejects_unknown_scenario', () => {
	const configuration: FixtureCommand = {
		surface: 'quota',
		binary: '/declared/test',
		root: "/owned/quoted ' root",
		capability: 'owned-capability',
		provider: '127.0.0.1:1234',
		temporary_directory: '/owned/home',
		output: '/owned/output',
	};
	const quota = childLaunchConfiguration(configuration);
	expect([quota.executable, ...quota.arguments]).toEqual([
		'/declared/test',
		'--router-root',
		"/owned/quoted ' root",
		'--fixture-capability',
		'owned-capability',
		'--provider-listener',
		'127.0.0.1:1234',
	]);
	expect(Object.keys(quota.environment).toSorted()).toEqual(['HOME', 'LANG', 'TERM', 'TMPDIR']);
	expect(quota.cwd).toBeUndefined();
	const sessions = childLaunchConfiguration({ ...configuration, surface: 'sessions' });
	expect([sessions.executable, ...sessions.arguments]).toEqual([
		'/declared/test',
		'--sessions-picker',
		'--provider-filter-storyboard',
		'--fixture-capability',
		'owned-capability',
	]);
	expect(() =>
		childLaunchConfiguration({
			...configuration,
			surface: 'quota-component',
			component_fixture_scenario: 'arbitrary',
		}),
	).toThrow('unknown component fixture scenario');
	expect(() => childLaunchConfiguration({ ...configuration, surface: 'other' })).toThrow(
		FixtureLaunchRejected,
	);
	expect(() => childLaunchConfiguration({ ...configuration, capability: '' })).toThrow(
		'missing fixture text: capability',
	);
});

test('launch_record_must_be_an_object_of_named_text_fields', () => {
	const directory = temporaryDirectory();
	const path = join(directory, 'configuration.json');
	for (const [text, message] of [
		['["surface"]', 'must be an object'],
		['null', 'must be an object'],
		['{"surface":"quota","binary":1}', 'named text fields'],
	] as const) {
		writeFileSync(path, text);
		expect(() => readFixtureCommand(path)).toThrow(message);
	}
	writeFileSync(path, '{"surface":"quota","note":"quote\\" and \\u0001 control"}');
	expect(readFixtureCommand(path)).toEqual({ surface: 'quota', note: 'quote" and \u0001 control' });
});

function publicSessionsRecord(root: string): FixtureCommand {
	const output = join(root, 'output');
	const cwd = join(root, 'approved source');
	const debugRoot = join(root, 'owned debug root');
	mkdirSync(output);
	mkdirSync(cwd);
	mkdirSync(join(debugRoot, 'agent-communication'), { recursive: true });
	writeFileSync(
		join(debugRoot, 'agent-communication/service.json'),
		JSON.stringify({
			serviceId: '00000000-0000-4000-8000-000000000001',
			serviceEpoch: '{00000000-0000-4000-8000-000000000002}',
		}),
	);
	const binary = join(root, 'controlled public entry');
	writeFileSync(binary, '#!/bin/sh\nexit 0\n');
	chmodSync(binary, 0o700);
	return {
		surface: 'public-sessions',
		binary,
		binary_sha256: createHash('sha256').update('#!/bin/sh\nexit 0\n').digest('hex'),
		approved_cwd: cwd,
		debug_router_root: debugRoot,
		expected_service_id: '00000000-0000-4000-8000-000000000001',
		expected_service_epoch: '00000000-0000-4000-8000-000000000002',
		output,
		ttyd: 'unused',
		temporary_directory: root,
		dynamic_library_path: 'unused',
	};
}

test('public_sessions_launch_runs_only_the_bound_binary_in_its_approved_cwd', () => {
	const root = temporaryDirectory();
	const record = publicSessionsRecord(root);
	const launch = childLaunchConfiguration(record);
	expect(launch.executable).toBe(join(root, 'controlled public entry'));
	expect(launch.arguments).toEqual([]);
	expect(launch.cwd).toBe(join(root, 'approved source'));
	expect(launch.environment['CODEX_ROUTER_DEBUG_ROUTER_ROOT']).toBe(join(root, 'owned debug root'));
	expect(launch.environment['TERM']).toBe('xterm-256color');
	expect(launch.environment['LANG']).toBe('C.UTF-8');
});

test('public_sessions_launch_rejects_changed_bindings_before_any_child', () => {
	const root = temporaryDirectory();
	const record = publicSessionsRecord(root);
	const linkedCwd = join(root, 'linked source');
	symlinkSync(join(root, 'approved source'), linkedCwd);
	for (const [change, message] of [
		[{ expected_service_epoch: '00000000-0000-4000-8000-000000000003' }, 'service/epoch mismatch'],
		[{ binary_sha256: '0'.repeat(64) }, 'binary binding changed'],
		[{ arguments: '--new' }, 'exact bound configuration fields'],
		[{ approved_cwd: linkedCwd }, 'directory binding changed'],
		[{ binary: join(root, 'missing entry') }, 'binary binding changed'],
	] as const) {
		expect(() => childLaunchConfiguration({ ...record, ...change })).toThrow(message);
	}
	const manifest = join(root, 'owned debug root/agent-communication/service.json');
	rmSync(manifest);
	writeFileSync(join(root, 'elsewhere.json'), '{}');
	symlinkSync(join(root, 'elsewhere.json'), manifest);
	expect(() => childLaunchConfiguration(record)).toThrow('Host manifest location changed');
});

test('launch record path is the environment record repeated as the single argument', () => {
	const path = '/owned/output/fixture-command.json';
	expect(fixtureCommandPath([path], path)).toBe(path);
	expect(() => fixtureCommandPath([path], undefined)).toThrow(
		'QUOTA_STORYBOARD_FIXTURE_COMMAND is required',
	);
	for (const commandArguments of [[], ['relative/fixture-command.json'], [path, '--extra']]) {
		expect(() => fixtureCommandPath(commandArguments, path)).toThrow(
			'exactly one absolute fixture command path',
		);
	}
	expect(() => fixtureCommandPath([path], '/other/output/fixture-command.json')).toThrow(
		'must match QUOTA_STORYBOARD_FIXTURE_COMMAND',
	);
});
