import {
	accessSync,
	chmodSync,
	constants,
	existsSync,
	lstatSync,
	mkdirSync,
	realpathSync,
	statSync,
	writeFileSync,
} from 'node:fs';
import { isAbsolute, join, resolve } from 'node:path';

import { z } from 'zod';

import { captureEnvironment } from './capture-execution.ts';
import { requireOneTest } from './capture-selection.ts';
import { readCuaState } from './cua-readback.ts';
import { discoverCaptureTest, startDriver } from './driver-lifecycle.ts';
import {
	artifactBindingSchema,
	capturePlanSchema,
	processIdentitySchema,
} from './evidence-schema.ts';
import type { CuaPtyReadback, TerminalViewport } from './evidence-schema.ts';
import { collectCuaArtifacts } from './original-artifacts.ts';
import { lookupProcessIdentity, sameIdentity } from './process-identity.ts';
import { waitPublicDriver } from './public-lifecycle.ts';
import { captureTest, supportDirectory } from './scene-contracts.ts';
import {
	captureArtifact,
	fileDigest,
	readJson,
	verifyArtifact,
	verifyTarget,
} from './source-binding.ts';
export const publicHostIdentitySchema = z
	.strictObject({
		service_id: z.uuid(),
		service_epoch: z.uuid(),
	})
	.readonly();

export type PublicHostIdentity = z.infer<typeof publicHostIdentitySchema>;
export const publicTargetSchema = z
	.strictObject({
		binary: artifactBindingSchema,
		approved_cwd: z.string().refine(isAbsolute, 'public Sessions directories must be absolute'),
		debug_router_root: z
			.string()
			.refine(isAbsolute, 'public Sessions directories must be absolute'),
		expected_host: publicHostIdentitySchema,
	})
	.readonly();

export type PublicSessionsTarget = z.infer<typeof publicTargetSchema>;
export const runtimePlanSchema = z
	.strictObject({
		repository: z.string(),
		source_head: z.string(),
		qualification: z.string(),
		router_root: z.string(),
		evidence: z.string(),
		port: z.number().int().positive().max(65535),
		original_port: z.number().int().positive().max(65535),
		expected_version: z.string(),
		profile: z.string(),
		claude_node: z.string(),
		claude_adapter: z.string(),
		claude_executable: z.string(),
		cursor_executable: z.string(),
		deadline: z.iso.datetime({ offset: true, local: true }),
	})
	.readonly();

export const publicPlanSchema = capturePlanSchema
	.unwrap()
	.extend({
		public_target: publicTargetSchema,
		runtime_plan: runtimePlanSchema,
		runtime_effect_evidence: artifactBindingSchema,
	})
	.readonly();

export type PublicSessionsPlan = z.infer<typeof publicPlanSchema>;
export const publicLifecycleSchema = z
	.strictObject({
		kind: z.literal('public-sessions-cua-lifecycle-only'),
		binary_sha256: z.string(),
		node_sha256: artifactBindingSchema.unwrap().shape.sha256,
		public_capture_source_sha256: z.string(),
		fixed_child_sha256: z.string(),
		host_service_id: z.uuid(),
		host_service_epoch: z.uuid(),
		child_exit: z.literal(0),
		owned_processes_stopped: z.literal(true),
		artifact_sink_stopped: z.literal(true),
		artifact_count: z.number().int().nonnegative(),
		last_viewport: z.string(),
		effect_evidence_sha256: z.string(),
		job_settlement: z.literal('separate-runtime-helper-proof-required'),
	})
	.readonly();

const hostManifestSchema = z.looseObject({
	serviceId: z.string(),
	serviceEpoch: z.string(),
	mcp: z.looseObject({ url: z.string() }),
	serviceVersion: z.string(),
	machineLabel: z.string(),
	routerProxyEndpoint: z.string(),
});
export function requirePublicTarget(
	target: PublicSessionsTarget,
	observed: PublicHostIdentity,
): void {
	if (
		target.expected_host.service_id !== observed.service_id ||
		target.expected_host.service_epoch !== observed.service_epoch
	)
		throw new Error('public Sessions Host service/epoch mismatch');
	verifyArtifact(target.binary);
	accessSync(target.binary.path, constants.X_OK);
	if (!statSync(target.binary.path).isFile())
		throw new Error('bound public Sessions executable required');
	for (const directory of [target.approved_cwd, target.debug_router_root])
		if (!statSync(directory).isDirectory() || realpathSync(directory) !== directory)
			throw new Error('public Sessions directory binding changed');
}
export function publicSessionsLaunchConfiguration(
	target: PublicSessionsTarget,
	observed: PublicHostIdentity,
	inherited: Readonly<NodeJS.ProcessEnv>,
): { readonly arguments: string[]; readonly environment: NodeJS.ProcessEnv } {
	requirePublicTarget(target, observed);
	return {
		arguments: [target.binary.path],
		environment: { ...inherited, CODEX_ROUTER_DEBUG_ROUTER_ROOT: target.debug_router_root },
	};
}
export function publicSessionsChildConfiguration(
	target: PublicSessionsTarget,
	observed: PublicHostIdentity,
	output: string,
): Record<string, string> {
	requirePublicTarget(target, observed);
	if (
		!isAbsolute(output) ||
		resolve(output) !== output ||
		!statSync(join(output, '..')).isDirectory() ||
		(existsSync(output) && (lstatSync(output).isSymbolicLink() || !statSync(output).isDirectory()))
	)
		throw new Error('public Sessions requires its owned output directory');
	return {
		surface: 'public-sessions',
		binary: target.binary.path,
		binary_sha256: target.binary.sha256,
		approved_cwd: target.approved_cwd,
		debug_router_root: target.debug_router_root,
		expected_service_id: target.expected_host.service_id,
		expected_service_epoch: target.expected_host.service_epoch,
		output,
	};
}
export async function qualifyPublicTarget(plan: PublicSessionsPlan): Promise<void> {
	verifyTarget(plan);
	if (
		plan.harness_binary.path !== plan.public_target.binary.path ||
		plan.harness_binary.sha256 !== plan.public_target.binary.sha256
	)
		throw new Error('public CUA target executable binding mismatch');
	verifyArtifact(plan.runtime_effect_evidence);
	if (plan.runtime_plan.router_root !== plan.public_target.debug_router_root)
		throw new Error('public Sessions runtime root mismatch');
	const recorded = processIdentitySchema
		.unwrap()
		.extend({ parent: z.number().int() })
		.parse(readJson(join(plan.runtime_plan.evidence, 'router-identity.json')));
	if (!sameIdentity(lookupProcessIdentity(recorded.pid), recorded))
		throw new Error('live Router process identity mismatch');
	const started = z
		.looseObject({ binarySHA256: z.string() })
		.parse(readJson(join(plan.runtime_plan.evidence, 'router-started.json')));
	if (fileDigest(recorded.executable) !== started.binarySHA256)
		throw new Error('live Router binary changed');
	const manifest = hostManifestSchema.parse(
		readJson(join(plan.runtime_plan.router_root, 'agent-communication/service.json')),
	);
	if (
		manifest.serviceVersion !== plan.runtime_plan.expected_version ||
		manifest.routerProxyEndpoint !== `127.0.0.1:${plan.runtime_plan.port}`
	)
		throw new Error('published Host root, version or Router endpoint mismatch');
	const response = await fetch(`http://127.0.0.1:${plan.runtime_plan.port}/healthz`, {
		signal: AbortSignal.timeout(1000),
	});
	if (!response.ok) throw new Error(`health response status ${response.status}`);
	const health = z.looseObject({ binary_version: z.string() }).parse(await response.json());
	if (health.binary_version !== plan.runtime_plan.expected_version)
		throw new Error('Router version mismatch');
	requirePublicTarget(
		plan.public_target,
		publicHostIdentitySchema.parse({
			service_id: manifest.serviceId,
			service_epoch: manifest.serviceEpoch,
		}),
	);
}
export async function preparePublicCapture(plan: PublicSessionsPlan): Promise<NodeJS.ProcessEnv> {
	await qualifyPublicTarget(plan);
	if (plan.cua_artifact_scenes.length === 0)
		throw new Error('public capture needs exact declared original scenes');
	requireOneTest(
		await discoverCaptureTest(
			[plan.capture_driver.path, captureTest, '--exact', '--list'],
			plan.repository,
		),
		captureTest,
	);
	mkdirSync(plan.output, { mode: 0o700 });
	const configuration = {
		...publicSessionsChildConfiguration(
			plan.public_target,
			plan.public_target.expected_host,
			join(plan.output, 'cua-sessions'),
		),
		ttyd: plan.ttyd.path,
		temporary_directory: plan.temporary_directory,
		dynamic_library_path: plan.dynamic_library_path,
	};
	const targetFile = join(plan.output, 'public-target.json');
	writeFileSync(targetFile, JSON.stringify(configuration), { mode: 0o600, flag: 'wx' });
	chmodSync(targetFile, 0o600);
	writeFileSync(join(plan.output, 'declared-target.json'), JSON.stringify(plan, null, 2));
	return {
		QUOTA_STORYBOARD_PUBLIC_SESSIONS_CONFIG: targetFile,
		QUOTA_STORYBOARD_PUBLIC_SESSIONS_CONFIG_SHA256: captureArtifact(targetFile).sha256,
		QUOTA_STORYBOARD_PUBLIC_SESSIONS_BINARY: plan.public_target.binary.path,
		QUOTA_STORYBOARD_PUBLIC_SESSIONS_SERVICE_ID: plan.public_target.expected_host.service_id,
		QUOTA_STORYBOARD_PUBLIC_SESSIONS_SERVICE_EPOCH: plan.public_target.expected_host.service_epoch,
		QUOTA_STORYBOARD_PUBLIC_SESSIONS_EFFECT_SHA256: plan.runtime_effect_evidence.sha256,
	};
}
export async function runPublicCapture(plan: PublicSessionsPlan): Promise<void> {
	const launch = await preparePublicCapture(plan);
	const environment = {
		...captureEnvironment(plan),
		...launch,
		QUOTA_STORYBOARD_CUA_SURFACE: 'sessions',
		QUOTA_STORYBOARD_CUA_ARTIFACT_SCENES: plan.cua_artifact_scenes.join(','),
	};
	const command = [
		plan.capture_driver.path,
		captureTest,
		'--exact',
		'--ignored',
		'--nocapture',
		'--test-threads=1',
	];
	writeFileSync(join(plan.output, 'command.json'), JSON.stringify(command));
	// Native driver owns its PTY, browser, and native/provider job. The only
	// cancellation is its pre-existing private cooperative marker.
	const { openSync, closeSync } = await import('node:fs');
	const stdout = openSync(join(plan.output, 'driver.stdout'), 'w');
	const stderr = openSync(join(plan.output, 'driver.stderr'), 'w');
	try {
		const child = startDriver(command, {
			cwd: plan.repository,
			env: environment,
			stdio: ['ignore', stdout, stderr],
		});
		await waitPublicDriver(child, plan.output);
	} finally {
		closeSync(stdout);
		closeSync(stderr);
	}
	await qualifyPublicTarget(plan);
	const receipt = publicLifecycleSchema.parse(
		readJson(join(plan.output, 'cua-sessions/public-cua-receipt.json')),
	);
	if (
		receipt.node_sha256 !== plan.node.sha256 ||
		receipt.binary_sha256 !== plan.public_target.binary.sha256 ||
		receipt.public_capture_source_sha256 !==
			fileDigest(join(plan.repository, supportDirectory, 'public_sessions_capture.rs')) ||
		receipt.fixed_child_sha256 !==
			fileDigest(join(plan.repository, 'scripts/verification/tui/fixture-child.ts')) ||
		receipt.host_service_id !== plan.public_target.expected_host.service_id ||
		receipt.host_service_epoch !== plan.public_target.expected_host.service_epoch ||
		receipt.effect_evidence_sha256 !== plan.runtime_effect_evidence.sha256
	)
		throw new Error('public CUA lifecycle source/Host/effect binding mismatch');
	writeFileSync(
		join(plan.output, 'public-cua-summary.json'),
		JSON.stringify(
			{
				target: plan,
				receipt,
				original_artifacts: collectCuaArtifacts(plan, 'sessions', receipt.artifact_count),
				complete: false,
				limits:
					'UI originals and actual Host/job effects/settlement require separate observation; no fixture zero-effect claim.',
			},
			null,
			2,
		),
	);
}

export async function readPublicCuaState(
	plan: PublicSessionsPlan,
	viewport: TerminalViewport,
): Promise<CuaPtyReadback> {
	await qualifyPublicTarget(plan);
	return readCuaState(plan, 'sessions', viewport);
}
