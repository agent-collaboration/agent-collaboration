import { spawn } from 'node:child_process';
/** Controlled native-adapter protocol stand-in. This program never renders product UI. */
import {
	chmodSync,
	existsSync,
	mkdirSync,
	readFileSync,
	realpathSync,
	watch,
	writeFileSync,
} from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { z } from 'zod';

import { isMain, reportCliFailure } from './cli-arguments.ts';
import { captureArtifact } from './source-binding.ts';

const modeSchema = z.enum([
	'valid',
	'missing-receipt',
	'nonzero-child',
	'forged-child-marker',
	'pending-valid',
	'pending-missing',
	'pending-wrong-release',
	'pending-wrong-receipt',
	'stale-receipt',
	'stale-release',
	'same-path',
	'quota',
	'sessions',
	'unknown-scenario',
	'unknown-surface',
	'public',
	'public-held',
	'public-held-sibling',
	'public-wrong-host',
	'public-wrong-binary',
	'public-extra-argument',
]);
const controlSchema = z
	.strictObject({
		mode: modeSchema,
		root: z.string().refine(isAbsolute),
		node: z.string().refine(isAbsolute),
	})
	.readonly();
function save(path: string, value: unknown): void {
	writeFileSync(path, JSON.stringify(value), { flag: 'wx', mode: 0o600 });
}
export function prepareNativeAdapterFixture(root: string, mode: string, node: string): void {
	const control = controlSchema.parse({ root: realpathSync(root), mode, node: realpathSync(node) });
	const output = join(root, 'output');
	const home = join(root, 'home');
	const approved = join(root, 'approved source');
	const debug = join(root, 'debug root');
	for (const directory of [output, home, approved, join(debug, 'agent-communication')])
		mkdirSync(directory, { recursive: true, mode: 0o700 });
	save(join(root, 'control.json'), control);
	const binary = join(root, "controlled ' public entry.mjs");
	writeFileSync(
		binary,
		`#!${control.node}\nimport {nativeAdapterFixtureMain} from ${JSON.stringify(import.meta.url)};\nnativeAdapterFixtureMain(['child',${JSON.stringify(control.root)},...process.argv.slice(2)]);\n`,
		{ flag: 'wx', mode: 0o700 },
	);
	chmodSync(binary, 0o700);
	const pending = mode.startsWith('pending-') || mode.startsWith('stale-') || mode === 'same-path';
	const receipt = join(output, 'fixture-finished');
	const release = join(output, 'fixture-release');
	const configuration: Record<string, string> = {
		binary,
		surface: 'quota-component',
		output,
		temporary_directory: home,
		component_fixture_scenario: pending ? 'commit-pending' : 'normal',
		component_fixture_receipt: receipt,
	};
	if (pending)
		configuration['component_fixture_release'] = mode === 'same-path' ? receipt : release;
	if (mode === 'stale-receipt') writeFileSync(receipt, 'stale\n');
	if (mode === 'stale-release') writeFileSync(release, 'settle\n');
	if (mode === 'quota' || mode === 'sessions') {
		configuration['surface'] = mode;
		configuration['root'] = "/owned/quoted ' root";
		configuration['capability'] = 'owned-capability';
		configuration['provider'] = '127.0.0.1:1234';
	}
	if (mode === 'unknown-scenario') configuration['component_fixture_scenario'] = 'arbitrary';
	if (mode === 'unknown-surface') configuration['surface'] = 'other';
	if (mode.startsWith('public')) {
		for (const key of Object.keys(configuration)) delete configuration[key];
		Object.assign(configuration, {
			surface: 'public-sessions',
			binary,
			binary_sha256: captureArtifact(binary).sha256,
			approved_cwd: approved,
			debug_router_root: debug,
			expected_service_id: '00000000-0000-4000-8000-000000000001',
			expected_service_epoch: '00000000-0000-4000-8000-000000000002',
			output,
			ttyd: 'unused',
			temporary_directory: home,
			dynamic_library_path: 'unused',
		});
		save(join(debug, 'agent-communication/service.json'), {
			serviceId: '00000000-0000-4000-8000-000000000001',
			serviceEpoch: '00000000-0000-4000-8000-000000000002',
		});
		if (mode === 'public-wrong-host')
			configuration['expected_service_epoch'] = '00000000-0000-4000-8000-000000000003';
		if (mode === 'public-wrong-binary') configuration['binary_sha256'] = '0'.repeat(64);
		if (mode === 'public-extra-argument') configuration['arguments'] = '--new';
	}
	save(join(root, 'configuration.json'), configuration);
}
function runControlledChild(root: string, argumentsList: readonly string[]): void {
	const raw: unknown = JSON.parse(readFileSync(join(root, 'control.json'), 'utf8'));
	const control = controlSchema.parse(raw);
	const output = join(root, 'output');
	writeFileSync(join(output, 'arguments.txt'), argumentsList.join('\n'));
	writeFileSync(join(output, 'scenario.txt'), process.env['QUOTA_COMPONENT_FIXTURE'] ?? 'absent');
	writeFileSync(join(output, 'cwd.txt'), process.cwd());
	writeFileSync(join(output, 'home.txt'), process.env['HOME'] ?? 'absent');
	writeFileSync(join(output, 'codex-home.txt'), process.env['CODEX_HOME'] ?? 'absent');
	writeFileSync(join(output, 'sentinel.txt'), process.env['PUBLIC_TEST_SENTINEL'] ?? 'absent');
	writeFileSync(
		join(output, 'router-root.txt'),
		process.env['CODEX_ROUTER_DEBUG_ROUTER_ROOT'] ?? 'absent',
	);
	writeFileSync(
		join(output, 'environment-keys.txt'),
		Object.keys(process.env).toSorted().join('\n'),
	);
	save(join(output, 'standin-identity.json'), {
		pid: process.pid,
		parent: process.ppid,
		uid: process.getuid?.(),
		node: realpathSync(process.execPath),
	});
	const finish = (): void => {
		if (control.mode === 'forged-child-marker')
			process.stdout.write('QUOTA_STORYBOARD_CHILD_EXIT=0\n');
		const receipt = process.env['QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE'];
		const pending = control.mode.startsWith('pending-');
		if (
			receipt !== undefined &&
			control.mode !== 'missing-receipt' &&
			control.mode !== 'forged-child-marker'
		) {
			if (pending && control.mode !== 'pending-valid' && control.mode !== 'pending-missing')
				writeFileSync(
					join(output, 'fixture-release'),
					control.mode === 'pending-wrong-release' ? 'wrong\n' : 'settle\n',
				);
			writeFileSync(
				receipt,
				`QUOTA_COMPONENT_FIXTURE_FINISHED scenario=${pending && control.mode !== 'pending-wrong-receipt' ? 'CommitPending' : 'Normal'} released=${String(pending)} external_requests=0 persisted_writes=0\n`,
			);
		}
		process.exitCode = control.mode === 'nonzero-child' ? 7 : 0;
	};
	for (let line = 0; line < 40; line += 1)
		process.stdout.write(`controlled scrollback line ${line}\n`);
	if (control.mode === 'public-held-sibling') {
		const bystander = spawn(process.execPath, [fileURLToPath(import.meta.url), 'bystander', root], {
			stdio: 'ignore',
		});
		bystander.unref();
	}
	if (control.mode === 'pending-valid' || control.mode.startsWith('public-held')) {
		const release =
			control.mode === 'pending-valid'
				? join(output, 'fixture-release')
				: join(root, 'release-child');
		const watcher = watch(control.mode === 'pending-valid' ? output : root, () => {
			if (
				existsSync(release) &&
				readFileSync(release, 'utf8') ===
					(control.mode === 'pending-valid' ? 'settle\n' : 'release\n')
			) {
				watcher.close();
				finish();
			}
		});
		process.stdout.write('NATIVE_ADAPTER_READY\n');
	} else finish();
}
export function nativeAdapterFixtureMain(args: readonly string[]): void {
	const [operation, root, mode, node, ...extra] = args;
	if (
		operation === 'prepare' &&
		root !== undefined &&
		mode !== undefined &&
		node !== undefined &&
		extra.length === 0
	) {
		prepareNativeAdapterFixture(root, mode, node);
		return;
	}
	if (operation === 'bystander' && root !== undefined) {
		const watcher = watch(root, () => {
			const release = join(root, 'release-bystander');
			if (existsSync(release) && readFileSync(release, 'utf8') === 'release\n') watcher.close();
		});
		writeFileSync(join(root, 'output/bystander.pid'), String(process.pid));
		return;
	}
	if (operation === 'child' && root !== undefined) {
		runControlledChild(root, args.slice(2));
		return;
	}
	throw new Error('native-adapter-fixture.ts prepare ROOT MODE NODE | child ROOT [declared argv]');
}
if (isMain(import.meta.url)) {
	try {
		nativeAdapterFixtureMain(process.argv.slice(2));
	} catch (error) {
		reportCliFailure(error);
	}
}
