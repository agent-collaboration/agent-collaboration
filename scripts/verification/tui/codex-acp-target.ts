/**
 * The installed Codex CLI target for the Codex ACP journeys (J1 skills, J2 commands, J3
 * preservation): which unchanged executable runs, which isolated Host socket and provider
 * Session it resumes, and the exact argv the shared PTY driver launches. Renderer-independent;
 * the PTY driver and the wterm grid consume the launch record this module qualifies.
 */
import { spawnSync } from 'node:child_process';
import { lstatSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { isAbsolute, join } from 'node:path';

import { z } from 'zod';

// The one alias rule, owned by the same-session observer's pure oracle.
import { nativeThreadAlias } from '../../../verification/observations/acp-journey-oracle.ts';
import { artifactBindingSchema, viewportSchema } from './evidence-schema.ts';
import { readJson, verifyArtifact } from './source-binding.ts';

const absoluteDirectorySchema = z
	.string()
	.refine(isAbsolute, 'Codex ACP target directories must be absolute');
export const codexAcpEndpointSchema = z.enum(['claude-local', 'cursor-local']);
export type CodexAcpEndpoint = z.infer<typeof codexAcpEndpointSchema>;

export const codexAcpTargetSchema = z
	.strictObject({
		codex: artifactBindingSchema,
		codex_version: z.literal('0.165.0'),
		service_directory: absoluteDirectorySchema,
		expected_service_id: z.uuid(),
		expected_service_epoch: z.uuid(),
		endpoint_id: codexAcpEndpointSchema,
		// The raw provider SessionRef session ID (opaque provider data, not necessarily a UUID).
		provider_session_id: z.string().min(1),
		// The Router's native alias for it, the thread Codex resumes. The supporting observer plan
		// checks this pair against the alias rule; this target only resumes and correlates it.
		native_thread_id: z.uuid(),
		// The trusted working directory the TUI passes with --cd; never created or trusted here.
		cwd: absoluteDirectorySchema,
		alternate_screen: z.boolean(),
		viewport: viewportSchema,
		// The same-session supporting observer plan (verification/observations/acp-journey-models.ts).
		observer_plan: absoluteDirectorySchema,
	})
	.readonly();
export type CodexAcpTarget = z.infer<typeof codexAcpTargetSchema>;

export interface CodexLaunchRecord {
	readonly program: string;
	readonly arguments: readonly string[];
	readonly cwd: string;
}

export function codexSocketPath(target: CodexAcpTarget): string {
	return join(target.service_directory, 'router-sessions', `${target.endpoint_id}.sock`);
}

/**
 * The exact launch the existing ACP recipe uses: unchanged Codex resuming one Router Session
 * over the endpoint's private Unix socket, with hooks off and normal Codex home inherited.
 */
export function codexLaunchRecord(target: CodexAcpTarget): CodexLaunchRecord {
	return {
		program: target.codex.path,
		arguments: [
			...(target.alternate_screen ? [] : ['--no-alt-screen']),
			'--remote',
			`unix://${codexSocketPath(target)}`,
			'--cd',
			target.cwd,
			'-c',
			'features.hooks=false',
			'resume',
			target.native_thread_id,
		],
		cwd: target.cwd,
	};
}

const hostManifestSchema = z.looseObject({ serviceId: z.uuid(), serviceEpoch: z.uuid() });
const observerCorrelationSchema = z.looseObject({
	codex_cli: z.string(),
	codex_sha256: z.string(),
	service_directory: z.string(),
	endpoint_id: z.string(),
	session_id: z.string(),
	native_thread_id: z.string(),
	cwd: z.string(),
});

function stableDirectory(path: string, label: string): void {
	if (!statSync(path).isDirectory() || realpathSync(path) !== path)
		throw new Error(`${label} directory binding changed`);
}

/** Checks every declared identity before the driver may launch the child. */
export function qualifyCodexAcpTarget(target: CodexAcpTarget): CodexLaunchRecord {
	verifyArtifact(target.codex);
	const version = spawnSync(target.codex.path, ['--version'], { encoding: 'utf8', timeout: 5000 });
	if (version.status !== 0 || version.stdout.trim() !== `codex-cli ${target.codex_version}`)
		throw new Error('installed Codex version differs from the declared target');
	const production = join(homedir(), '.codex-router');
	if (
		target.service_directory === production ||
		target.service_directory.startsWith(`${production}/`)
	)
		throw new Error('production Router service is outside the Codex ACP journeys');
	stableDirectory(target.service_directory, 'service');
	stableDirectory(target.cwd, 'trusted cwd');
	const manifest = hostManifestSchema.parse(
		JSON.parse(readFileSync(join(target.service_directory, 'service.json'), 'utf8')),
	);
	if (
		manifest.serviceId !== target.expected_service_id ||
		manifest.serviceEpoch !== target.expected_service_epoch
	)
		throw new Error('isolated Host service/epoch mismatch');
	// Codex must resume the exact native alias of this raw provider Session on this Host endpoint.
	if (
		nativeThreadAlias(manifest.serviceId, target.endpoint_id, target.provider_session_id) !==
		target.native_thread_id
	)
		throw new Error('native thread is not the Router alias of this provider Session');
	const socket = codexSocketPath(target);
	const socketStatus = lstatSync(socket);
	if (!socketStatus.isSocket() || socketStatus.uid !== process.getuid?.())
		throw new Error('endpoint socket missing or not owned by this user');
	const observer = observerCorrelationSchema.parse(readJson(target.observer_plan));
	if (
		observer.codex_cli !== target.codex.path ||
		observer.codex_sha256 !== target.codex.sha256 ||
		observer.service_directory !== target.service_directory ||
		observer.endpoint_id !== target.endpoint_id ||
		observer.session_id !== target.provider_session_id ||
		observer.native_thread_id !== target.native_thread_id ||
		observer.cwd !== target.cwd
	)
		throw new Error('supporting observer plan addresses a different client or Session');
	return codexLaunchRecord(target);
}
