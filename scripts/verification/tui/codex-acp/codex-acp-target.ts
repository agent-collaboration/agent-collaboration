/**
 * The installed Codex CLI target for the Codex ACP journeys (J1 skills, J2 commands, J3
 * preservation): which unchanged executable runs, which isolated Host socket and provider
 * Session it resumes, and the exact argv the shared PTY driver launches. Renderer-independent;
 * the PTY driver and the wterm grid consume the launch record this module qualifies.
 */
import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, isAbsolute, join } from 'node:path';

import { z } from 'zod';

// The one alias rule, owned by the same-session observer's pure oracle.
import { nativeThreadAlias } from '../../../../verification/observations/providers/acp/acp-journey-oracle.ts';
import { artifactBindingSchema, viewportSchema } from '../evidence/evidence-schema.ts';
import { readJson, verifyArtifact } from '../evidence/source-binding.ts';

const absoluteDirectorySchema = z
	.string()
	.refine(isAbsolute, 'Codex ACP target directories must be absolute');
// codex-local is the Router's relay to real managed Codex: the O5 reference target.
/** The installed Codex CLI release these journeys are pinned to. */
export const CODEX_PINNED_VERSION = '0.166.0';

export const codexAcpEndpointSchema = z.enum(['claude-local', 'cursor-local', 'codex-local']);
export type CodexAcpEndpoint = z.infer<typeof codexAcpEndpointSchema>;

export const codexAcpTargetSchema = z
	.strictObject({
		codex: artifactBindingSchema,
		// The installed CLI is 0.166.0. The pinned scene text (CODEX_0_165_TEXT) was last checked
		// against Codex source at 360319d8 and is not re-checked for 0.166.0 here.
		codex_version: z.literal(CODEX_PINNED_VERSION),
		service_directory: absoluteDirectorySchema,
		expected_service_id: z.uuid(),
		expected_service_epoch: z.uuid(),
		endpoint_id: codexAcpEndpointSchema,
		// A Router target: the raw provider SessionRef session ID (opaque provider data, not
		// necessarily a UUID) and the Router's native alias for it, the thread Codex resumes. The
		// supporting observer plan checks this pair against the alias rule. Null for the real-Codex
		// reference, which starts its own thread (assignment Correction 29).
		provider_session_id: z.string().min(1).nullable(),
		native_thread_id: z.uuid().nullable(),
		// The real-Codex reference only: the model it starts with, passed on the command line so the
		// owner's default model never applies, and proved from the CLI's own thread/start reply on the
		// wire before any input. Null for a Router target.
		reference_model: z
			.strictObject({
				model: z.string().min(1),
				effort: z.string().min(1),
				// The managed Codex's model provider the reply must name (the debug profile's).
				model_provider: z.string().min(1),
			})
			.nullable()
			.default(null),
		// The real-Codex reference's second window only: the thread it resumes, the one window A's own
		// thread/start opened (review F2). Null for window A and for every Router target.
		attach_thread_id: z.uuid().nullable().default(null),
		// The trusted working directory the TUI passes with --cd; never created or trusted here. The
		// Session keeps its own cwd, which the observer plan declares and checks against native reads.
		cwd: absoluteDirectorySchema,
		alternate_screen: z.boolean(),
		viewport: viewportSchema,
		// The same-session supporting observer plan (verification/observations/providers/acp/acp-journey-models.ts).
		// Null only for codex-local: real Codex has no ACP Session for that observer to read.
		observer_plan: absoluteDirectorySchema.nullable(),
		// Footer items for this launch, in Codex's own status-line item ids. The default shows the
		// model alone. A comparison journey declares the model and a context-window item; the ids
		// are not grounded for 0.166.0 here, so the run plan declares them.
		// The startup bound (Resuming view, update modal and its dismissal) for this target. Real Codex
		// resumes a native thread more slowly than the Router's provider face (D0 r1 attempt 3).
		startup_deadline_ms: z.number().int().positive().max(120_000).default(8_000),
		status_line: z
			.array(z.string().regex(/^[a-z][a-z0-9-]*$/u))
			.min(1)
			.default(['model-with-reasoning']),
		// A wire tee between the CLI and the endpoint socket (O5): the CLI connects to listen_socket
		// and the tee relays to the endpoint socket, recording every frame.
		wire_tee: z
			.strictObject({
				// Must not exist at qualification; short enough for a Unix socket path.
				listen_socket: absoluteDirectorySchema.refine(
					(path) => Buffer.byteLength(path) < 100,
					'tee socket path exceeds the Unix socket limit',
				),
				// An absolute JSON-lines path under the run output (the run plan checks the output).
				recording: absoluteDirectorySchema.refine(
					(path) => path.endsWith('.jsonl'),
					'tee recording is a .jsonl file',
				),
			})
			.nullable()
			.default(null),
	})
	.refine(
		(target) => (target.observer_plan === null) === (target.endpoint_id === 'codex-local'),
		'only a codex-local target has no observer plan, and every other endpoint requires one',
	)
	.refine(
		(target) =>
			target.endpoint_id === 'codex-local'
				? target.provider_session_id === null &&
					target.native_thread_id === null &&
					target.reference_model !== null
				: target.provider_session_id !== null &&
					target.native_thread_id !== null &&
					target.reference_model === null &&
					target.attach_thread_id === null,
		'the real-Codex reference starts its own thread with a declared model; a Router target resumes its declared Session',
	)
	.readonly();
export type CodexAcpTarget = z.infer<typeof codexAcpTargetSchema>;

/** A Router target's declared Session: the raw provider session and the thread alias Codex resumes. */
export interface RouterTargetSession {
	readonly providerSessionId: string;
	readonly nativeThreadId: string;
}

/** The declared Session of a Router target; the real-Codex reference has none (it starts fresh). */
export function routerSession(target: CodexAcpTarget): RouterTargetSession {
	if (target.provider_session_id === null || target.native_thread_id === null)
		throw new Error('the real-Codex reference starts its own thread and has no Router Session');
	return { providerSessionId: target.provider_session_id, nativeThreadId: target.native_thread_id };
}

export interface CodexLaunchRecord {
	readonly program: string;
	readonly arguments: readonly string[];
	readonly cwd: string;
}

/**
 * The socket the CLI attaches to. A provider endpoint (Claude, Cursor) is served by the Router's
 * session face at `router-sessions/<endpoint>.sock`; the real-Codex reference goes through N2's
 * byte relay, which the Host publishes as the `nativeCodex` channel `codex-native.sock` in the
 * service directory.
 */
export function codexSocketPath(target: CodexAcpTarget): string {
	return target.endpoint_id === 'codex-local'
		? join(target.service_directory, 'codex-native.sock')
		: join(target.service_directory, 'router-sessions', `${target.endpoint_id}.sock`);
}

/** The socket the CLI connects to: the tee's when a wire tee is declared, else the endpoint's. */
export function codexRemoteSocketPath(target: CodexAcpTarget): string {
	return target.wire_tee?.listen_socket ?? codexSocketPath(target);
}

/**
 * The exact launch the existing ACP recipe uses: unchanged Codex over the endpoint's private Unix
 * socket (or the wire tee in front of it), with hooks off and normal Codex home inherited. A Router
 * target resumes its declared Session. The real-Codex reference starts a new thread with its declared
 * model and effort: real Codex writes a thread's rollout only at its first user message, so a
 * turn-less thread cannot be resumed (assignment Correction 29).
 */
export function codexLaunchRecord(target: CodexAcpTarget): CodexLaunchRecord {
	// The reference's second window resumes the thread its first window started (review F2); that
	// thread's rollout exists once window A has sent its first message.
	const thread =
		target.reference_model === null
			? ['resume', routerSession(target).nativeThreadId]
			: target.attach_thread_id !== null
				? ['resume', target.attach_thread_id]
				: [
						'-m',
						target.reference_model.model,
						'-c',
						`model_reasoning_effort=${JSON.stringify(target.reference_model.effort)}`,
					];
	return {
		program: target.codex.path,
		arguments: [
			...(target.alternate_screen ? [] : ['--no-alt-screen']),
			'--remote',
			`unix://${codexRemoteSocketPath(target)}`,
			'--cd',
			target.cwd,
			'-c',
			'features.hooks=false',
			// Per-launch footer. The default is the model alone; a user status line may add
			// context-window-size, which Codex fills from its local config when the provider reports no
			// window (Cursor reports none). A comparison journey declares the window item as well.
			'-c',
			`tui.status_line=${JSON.stringify(target.status_line)}`,
			...thread,
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
});

function stableDirectory(path: string, label: string): void {
	if (!statSync(path).isDirectory() || realpathSync(path) !== path)
		throw new Error(`${label} directory binding changed`);
}

/**
 * Whether the declared wire tee's socket must be absent (the run plan check, before the run starts
 * the tee) or live and owned by this user (the launch wrapper, immediately before it spawns Codex).
 */
export type TeeSocketState = 'absent' | 'listening';

/** Checks every declared identity before the driver may launch the child. */
export function qualifyCodexAcpTarget(
	target: CodexAcpTarget,
	teeSocket: TeeSocketState = 'absent',
): CodexLaunchRecord {
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
	// A Router target must resume the exact native alias of its raw provider Session on this Host
	// endpoint. The real-Codex reference starts its own thread, which its run proves from the wire.
	if (target.reference_model === null) {
		const session = routerSession(target);
		if (
			nativeThreadAlias(manifest.serviceId, target.endpoint_id, session.providerSessionId) !==
			session.nativeThreadId
		)
			throw new Error('native thread is not the Router alias of this provider Session');
	}
	const socket = codexSocketPath(target);
	const socketStatus = lstatSync(socket);
	if (!socketStatus.isSocket() || socketStatus.uid !== process.getuid?.())
		throw new Error('endpoint socket missing or not owned by this user');
	if (target.wire_tee !== null) {
		const teePath = target.wire_tee.listen_socket;
		if (teeSocket === 'absent') {
			if (existsSync(teePath))
				throw new Error('tee listen socket already exists; choose a fresh private path');
			stableDirectory(dirname(teePath), 'tee socket');
		} else {
			const teeStatus = lstatSync(teePath);
			if (!teeStatus.isSocket() || teeStatus.uid !== process.getuid?.())
				throw new Error('tee socket missing or not owned by this user');
		}
	}
	if (target.observer_plan === null) return codexLaunchRecord(target);
	const observer = observerCorrelationSchema.parse(readJson(target.observer_plan));
	if (
		observer.codex_cli !== target.codex.path ||
		observer.codex_sha256 !== target.codex.sha256 ||
		observer.service_directory !== target.service_directory ||
		observer.endpoint_id !== target.endpoint_id ||
		observer.session_id !== target.provider_session_id ||
		observer.native_thread_id !== target.native_thread_id
	)
		throw new Error('supporting observer plan addresses a different client or Session');
	return codexLaunchRecord(target);
}
