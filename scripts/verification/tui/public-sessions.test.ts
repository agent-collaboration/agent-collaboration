import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { controlledExecutable, initializedSourceRepository } from './controlled-child-fixtures.ts';
import { lookupProcessIdentity } from './process-identity.ts';
import {
	publicPlanSchema,
	qualifyPublicTarget,
	checkPublicCapture,
	publicHostIdentitySchema,
	publicLifecycleSchema,
	publicTargetSchema,
	publicSessionsChildConfiguration,
	publicSessionsLaunchConfiguration,
} from './public-sessions.ts';
import type { PublicHostIdentity, PublicSessionsTarget } from './public-sessions.ts';
import { protocolPlan, saveJson, temporaryRoot } from './receipt-fixtures.ts';
import { captureArtifact, fileDigest, readJson, sourceBinding } from './source-binding.ts';
const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});
function publicFixture(): {
	root: string;
	approved: string;
	debug: string;
	record: string;
	executable: string;
	host: PublicHostIdentity;
	target: PublicSessionsTarget;
} {
	const root = temporaryRoot();
	roots.push(root);
	const approved = join(root, 'approved source');
	const debug = join(root, 'debug root');
	mkdirSync(approved);
	mkdirSync(debug);
	const record = join(root, 'invocation.json');
	const executable = controlledExecutable(
		root,
		'public entry',
		`const fs=require('node:fs');fs.writeFileSync(${JSON.stringify(record)},JSON.stringify({argv:process.argv.slice(2),cwd:process.cwd(),home:process.env.HOME,codex_home_present:Object.hasOwn(process.env,'CODEX_HOME'),codex_home:process.env.CODEX_HOME??null,root:process.env.CODEX_ROUTER_DEBUG_ROUTER_ROOT,sentinel:process.env.PUBLIC_TEST_SENTINEL}));`,
	);
	const host = publicHostIdentitySchema.parse({
		service_id: '00000000-0000-4000-8000-000000000001',
		service_epoch: '00000000-0000-4000-8000-000000000002',
	});
	const target = publicTargetSchema.parse({
		binary: captureArtifact(executable),
		approved_cwd: approved,
		debug_router_root: debug,
		expected_host: host,
	});
	return { root, approved, debug, record, executable, host, target };
}
const lifecycle = {
	kind: 'public-sessions-cua-lifecycle-only',
	node_sha256: '0'.repeat(64),
	binary_sha256: 'a'.repeat(64),
	public_capture_source_sha256: 'b'.repeat(64),
	fixed_child_sha256: 'c'.repeat(64),
	host_service_id: '00000000-0000-4000-8000-000000000001',
	host_service_epoch: '00000000-0000-4000-8000-000000000002',
	child_exit: 0,
	owned_processes_stopped: true,
	artifact_sink_stopped: true,
	artifact_count: 3,
	last_viewport: '48 12',
	effect_evidence_sha256: 'd'.repeat(64),
	job_settlement: 'separate-runtime-helper-proof-required',
};
test('public_lifecycle_cannot_claim_fixture_zero_effects_or_job_settlement', () => {
	publicLifecycleSchema.parse(lifecycle);
	for (const changed of [
		{ provider_requests: 0 },
		{ state_and_secrets_unchanged: true },
		{ job_settlement: 'settled' },
		{ child_exit: 1 },
		{ artifact_count: -1 },
		{ artifact_count: true },
	])
		expect(() => publicLifecycleSchema.parse({ ...lifecycle, ...changed })).toThrow();
	for (const missing of [
		'host_service_epoch',
		'effect_evidence_sha256',
		'public_capture_source_sha256',
		'node_sha256',
	])
		expect(() =>
			publicLifecycleSchema.parse(
				Object.fromEntries(Object.entries(lifecycle).filter(([key]) => key !== missing)),
			),
		).toThrow();
});
test.each([undefined, 'normal codex'])(
	'public_sessions_launch_uses_exact_binary_cwd_and_normal_environment %s',
	(codex) => {
		const fixture = publicFixture();
		const inherited: NodeJS.ProcessEnv = {
			HOME: join(fixture.root, 'normal home'),
			PUBLIC_TEST_SENTINEL: 'preserved',
		};
		if (codex !== undefined) inherited['CODEX_HOME'] = join(fixture.root, codex);
		const original = { ...inherited };
		const launch = publicSessionsLaunchConfiguration(fixture.target, fixture.host, inherited);
		expect(launch.arguments).toEqual([fixture.executable]);
		expect(inherited).toEqual(original);
		const result = spawnSync(fixture.executable, [], {
			cwd: fixture.approved,
			env: launch.environment,
			timeout: 2000,
		});
		expect(result.status).toBe(0);
		expect(readJson(fixture.record)).toEqual({
			argv: [],
			cwd: fixture.approved,
			home: inherited['HOME'],
			codex_home_present: codex !== undefined,
			codex_home: inherited['CODEX_HOME'] ?? null,
			root: fixture.debug,
			sentinel: 'preserved',
		});
	},
);
test.each(['service_id', 'service_epoch'])(
	'public launch rejects changed Host %s before child execution',
	(field) => {
		const fixture = publicFixture();
		expect(() =>
			publicSessionsLaunchConfiguration(
				fixture.target,
				{ ...fixture.host, [field]: '00000000-0000-4000-8000-000000000003' },
				{},
			),
		).toThrow('Host service/epoch mismatch');
	},
);
test('public launch refuses extra argv and changed binary, does not synthesize home', () => {
	const fixture = publicFixture();
	expect(() => publicTargetSchema.parse({ ...fixture.target, arguments: ['--new'] })).toThrow();
	const launch = publicSessionsLaunchConfiguration(fixture.target, fixture.host, {});
	expect(launch.environment).not.toHaveProperty('HOME');
	expect(launch.environment).not.toHaveProperty('CODEX_HOME');
	writeFileSync(fixture.executable, readFileSync(fixture.executable, 'utf8') + '\n//changed');
	expect(() => publicSessionsLaunchConfiguration(fixture.target, fixture.host, {})).toThrow(
		'artifact changed',
	);
});
test('public child configuration retains exact existing contract and rejects foreign output', () => {
	const fixture = publicFixture();
	const output = join(fixture.root, 'output');
	const configuration = publicSessionsChildConfiguration(fixture.target, fixture.host, output);
	expect(configuration).toEqual({
		surface: 'public-sessions',
		binary: fixture.executable,
		binary_sha256: fixture.target.binary.sha256,
		approved_cwd: fixture.approved,
		debug_router_root: fixture.debug,
		expected_service_id: fixture.host.service_id,
		expected_service_epoch: fixture.host.service_epoch,
		output,
	});
	expect(() => publicSessionsChildConfiguration(fixture.target, fixture.host, 'relative')).toThrow(
		'owned output',
	);
});

test('read-only Host qualification checks actual process binary manifest health and effect binding', async () => {
	const fixture = publicFixture();
	const repository = initializedSourceRepository();
	roots.push(repository);
	const base = protocolPlan(fixture.root);
	const discovery = controlledExecutable(
		fixture.root,
		'capture-discovery',
		"process.stdout.write('compiled_public_sessions_grid_observation: test\\n');",
	);
	const evidence = join(fixture.root, 'evidence');
	mkdirSync(evidence);
	mkdirSync(join(fixture.debug, 'agent-communication'));
	const identity = lookupProcessIdentity(process.pid);
	if (identity === null) throw new Error('controlled test process absent');
	saveJson(join(evidence, 'router-identity.json'), { ...identity, parent: process.ppid });
	saveJson(join(evidence, 'router-started.json'), {
		binarySHA256: fileDigest(identity.executable),
	});
	const effect = join(fixture.root, 'effects.json');
	writeFileSync(effect, 'separate runtime proof');
	let healthVersion = 'fixture-version';
	const server = createServer((_request, response) => {
		response.setHeader('Content-Type', 'application/json');
		response.end(JSON.stringify({ binary_version: healthVersion }));
	});
	await new Promise<void>((resolve) => {
		server.listen(0, '127.0.0.1', resolve);
	});
	try {
		const address = server.address();
		if (address === null || typeof address === 'string')
			throw new Error('controlled HTTP listener missing');
		const manifest = join(fixture.debug, 'agent-communication/service.json');
		const declaredManifest = {
			serviceId: fixture.host.service_id,
			serviceEpoch: fixture.host.service_epoch,
			mcp: { url: 'http://127.0.0.1/fixture' },
			serviceVersion: 'fixture-version',
			machineLabel: 'controlled',
			routerProxyEndpoint: `127.0.0.1:${address.port}`,
		};
		saveJson(manifest, declaredManifest);
		const plan = publicPlanSchema.parse({
			...base,
			capture_driver: captureArtifact(discovery),
			cua_artifact_scenes: ['browse'],
			chrome: base.capture_driver,
			keys: 'legacy',
			actions: [{ kind: 'checkpoint', scene: 'browse', pattern: 'Sessions' }],
			repository,
			source: sourceBinding(repository),
			harness_binary: fixture.target.binary,
			public_target: fixture.target,
			runtime_effect_evidence: captureArtifact(effect),
			runtime_plan: {
				repository,
				source_head: base.source.head,
				qualification: 'controlled fixture only',
				router_root: fixture.debug,
				evidence,
				port: address.port,
				original_port: address.port,
				expected_version: 'fixture-version',
				profile: 'fixture',
				claude_node: 'unused',
				claude_adapter: 'unused',
				claude_executable: 'unused',
				cursor_executable: 'unused',
				deadline: '2026-10-24T18:00:00Z',
			},
		});
		await qualifyPublicTarget(plan);
		await checkPublicCapture(plan);
		await expect(
			checkPublicCapture({ ...plan, actions: [...plan.actions, ...plan.actions] }),
		).rejects.toThrow('checkpoint sequence');
		healthVersion = 'wrong-version';
		await expect(qualifyPublicTarget(plan)).rejects.toThrow('Router version mismatch');
		healthVersion = 'fixture-version';
		saveJson(manifest, {
			...declaredManifest,
			serviceEpoch: '00000000-0000-4000-8000-000000000003',
		});
		await expect(qualifyPublicTarget(plan)).rejects.toThrow('Host service/epoch');
		saveJson(manifest, declaredManifest);
		saveJson(join(evidence, 'router-started.json'), { binarySHA256: '0'.repeat(64) });
		await expect(qualifyPublicTarget(plan)).rejects.toThrow('binary changed');
		saveJson(join(evidence, 'router-started.json'), {
			binarySHA256: fileDigest(identity.executable),
		});
		saveJson(join(evidence, 'router-identity.json'), {
			...identity,
			birth: 'reused pid',
			parent: process.ppid,
		});
		await expect(qualifyPublicTarget(plan)).rejects.toThrow('process identity');
		saveJson(join(evidence, 'router-identity.json'), { ...identity, parent: process.ppid });
		writeFileSync(effect, 'changed effects');
		await expect(qualifyPublicTarget(plan)).rejects.toThrow('artifact changed');
	} finally {
		await new Promise<void>((resolve, reject) => {
			server.close((error) => {
				if (error) reject(error);
				else resolve();
			});
		});
	}
});
