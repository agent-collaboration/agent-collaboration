/**
 * The real-Codex reference's model proof (assignment Correction 29). The reference starts its own
 * thread, so the model it runs is proved from the CLI's own `thread/start` reply on the wire tee,
 * before the driver types anything. The reply's shape is Codex's `ThreadStartResponse`
 * (`thread.id`, `model`, `modelProvider`, `reasoningEffort`), the same one the Router's provider
 * face returns for its own sessions.
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { z } from 'zod';

import { parseWireRecording } from '../../../../verification/observations/providers/acp/codex-wire-tee.ts';

export const REFERENCE_THREAD_START_FILE = 'reference-thread-start.json';

/** The model, effort and provider the reference must start with. */
export interface ReferenceModel {
	readonly model: string;
	readonly effort: string;
	readonly model_provider: string;
}

/** What the CLI's own thread/start reply reported. */
export interface ReferenceThreadStart {
	readonly connection: number;
	readonly thread_id: string;
	readonly model: string;
	readonly effort: string | null;
	readonly model_provider: string;
}

const requestSchema = z.looseObject({
	['id']: z.union([z.string(), z.number()]),
	method: z.string(),
});
const replySchema = z.looseObject({
	['id']: z.union([z.string(), z.number()]),
	result: z
		.looseObject({
			thread: z.looseObject({ ['id']: z.string().min(1) }),
			model: z.string(),
			modelProvider: z.string(),
			reasoningEffort: z.string().nullable(),
		})
		.optional(),
	error: z.unknown().optional(),
});

const titleThreadStartSchema = z.looseObject({
	params: z.looseObject({ threadSource: z.literal('thread_title') }),
});

/** Codex's own ephemeral title-generation thread, declared by its `threadSource` (Correction 33). */
function isTitleThreadStart(request: unknown): boolean {
	return titleThreadStartSchema.safeParse(request).success;
}

/** Where the CLI's own start stands in the recording: not yet there, or reported. */
export type ReferenceStartObservation =
	| { readonly kind: 'pending'; readonly reason: string }
	| { readonly kind: 'started'; readonly start: ReferenceThreadStart };

/**
 * The CLI's one thread/start and its reply, from the recording as it stands. Pending while the
 * recording, the start or its reply is not there yet; refuses at once a resume before that start
 * (a later scripted `/resume` is allowed), a second start or an error reply.
 */
export function observeReferenceThreadStart(recordingPath: string): ReferenceStartObservation {
	if (!existsSync(recordingPath)) return { kind: 'pending', reason: 'no wire recording yet' };
	const frames = parseWireRecording(readFileSync(recordingPath, 'utf8'));
	const requests = frames.flatMap((frame) => {
		if (frame.direction !== 'cliToServer') return [];
		const request = requestSchema.safeParse(frame.message);
		return request.success ? [{ connection: frame.connection, ...request.data }] : [];
	});
	// The reference opens its thread with its own start; a scripted /resume later in the run is a
	// journey action, not the launch (review F3).
	const opening = requests.find(
		(request) => request.method === 'thread/start' || request.method === 'thread/resume',
	);
	if (opening?.method === 'thread/resume')
		throw new Error('the real-Codex reference resumed a thread instead of starting one');
	// Codex's own title thread (`threadSource: "thread_title"`) is not a second start (Correction 33).
	const starts = requests.filter(
		(request) => request.method === 'thread/start' && !isTitleThreadStart(request),
	);
	const [first, ...others] = starts;
	if (others.length > 0) throw new Error('the real-Codex reference started more than one thread');
	if (first === undefined) return { kind: 'pending', reason: 'no thread/start yet' };
	const reply = frames.flatMap((frame) => {
		if (frame.direction !== 'serverToCli' || frame.connection !== first.connection) return [];
		const parsed = replySchema.safeParse(frame.message);
		return parsed.success && parsed.data.id === first.id ? [parsed.data] : [];
	})[0];
	if (reply === undefined) return { kind: 'pending', reason: 'no thread/start reply yet' };
	if (reply.result === undefined)
		throw new Error('the real-Codex reference has no successful thread/start reply');
	return {
		kind: 'started',
		start: {
			connection: first.connection,
			thread_id: reply.result.thread.id,
			model: reply.result.model,
			effort: reply.result.reasoningEffort,
			model_provider: reply.result.modelProvider,
		},
	};
}

/** The finished recording's start (after the run); a start still missing is a refusal. */
export function readReferenceThreadStart(recordingPath: string): ReferenceThreadStart {
	const observed = observeReferenceThreadStart(recordingPath);
	if (observed.kind === 'pending')
		throw new Error(
			`the real-Codex reference has no successful thread/start reply (${observed.reason})`,
		);
	return observed.start;
}

function requireDeclared(
	started: ReferenceThreadStart,
	declared: ReferenceModel,
): ReferenceThreadStart {
	if (
		started.model !== declared.model ||
		started.effort !== declared.effort ||
		started.model_provider !== declared.model_provider
	)
		throw new Error(
			`the real-Codex reference started ${started.model} / ${String(started.effort)} / ${started.model_provider}, not the declared ${declared.model} / ${declared.effort} / ${declared.model_provider}; no input sent`,
		);
	return started;
}

/**
 * Waits for the CLI's own successful thread/start reply, judging the recording again on every new
 * terminal frame up to the startup bound, then requires the declared start. Codex draws its empty
 * composer before its bootstrap reaches the tee (D0 r1 c29), so readiness is the reply, not the
 * composer. `waitForFrames` is the driver's bounded frame wait (or a test's stand-in).
 */
export async function awaitReferenceThreadStart(inputs: {
	readonly recordingPath: string;
	readonly declared: ReferenceModel;
	readonly boundMs: number;
	readonly waitForFrames: (matches: () => boolean, boundMs: number) => Promise<void>;
}): Promise<ReferenceThreadStart> {
	const observe = (): ReferenceStartObservation =>
		observeReferenceThreadStart(inputs.recordingPath);
	let observed = observe();
	if (observed.kind === 'pending') {
		await inputs
			.waitForFrames(() => {
				observed = observe();
				return observed.kind === 'started';
			}, inputs.boundMs)
			.catch((error: unknown) => {
				// A refusal thrown while judging a frame keeps its own message.
				if (error instanceof Error && error.message.startsWith('the real-Codex reference'))
					throw error;
				const pending = observe();
				if (pending.kind === 'started') return;
				throw new Error(
					`the real-Codex reference reported no successful thread/start within its startup bound (${pending.reason}); no input sent`,
					{ cause: error },
				);
			});
		observed = observe();
	}
	if (observed.kind === 'pending')
		throw new Error(
			`the real-Codex reference reported no successful thread/start (${observed.reason}); no input sent`,
		);
	return requireDeclared(observed.start, inputs.declared);
}

/** The finished recording: the reference started with exactly the declared model, effort and provider. */
export function requireReferenceThreadStart(
	recordingPath: string,
	declared: ReferenceModel,
): ReferenceThreadStart {
	return requireDeclared(readReferenceThreadStart(recordingPath), declared);
}

/** The run hook: wait for and prove the reference's start before any key, and keep what it reported. */
export function referenceModelProof(inputs: {
	readonly recordingPath: string;
	readonly declared: ReferenceModel;
	readonly output: string;
	readonly boundMs: number;
}): (waitForFrames: (matches: () => boolean, boundMs: number) => Promise<void>) => Promise<void> {
	return async (waitForFrames): Promise<void> => {
		const started = await awaitReferenceThreadStart({ ...inputs, waitForFrames });
		writeFileSync(
			join(inputs.output, REFERENCE_THREAD_START_FILE),
			JSON.stringify(started, null, 2),
			{
				mode: 0o600,
				flag: 'wx',
			},
		);
	};
}

/**
 * The reference's launch readiness for the startup handling: its own successful thread/start reply
 * is on the wire. A resumed thread, a second start or an error reply refuses at once.
 */
export function referenceLaunchReadiness(recordingPath: string): {
	readonly ready: () => boolean;
	readonly pending: () => string;
} {
	return {
		ready: (): boolean => observeReferenceThreadStart(recordingPath).kind === 'started',
		pending: (): string => {
			const observed = observeReferenceThreadStart(recordingPath);
			return observed.kind === 'pending' ? observed.reason : 'started';
		},
	};
}

/** What the reference's second window reported when it resumed window A's thread. */
export type ReferenceAttachObservation =
	| { readonly kind: 'pending'; readonly reason: string }
	| { readonly kind: 'attached'; readonly start: ReferenceThreadStart };

const resumeReplySchema = z.looseObject({
	['id']: z.union([z.string(), z.number()]),
	result: z
		.looseObject({
			thread: z.looseObject({ ['id']: z.string().min(1) }),
			model: z.string(),
			modelProvider: z.string(),
			reasoningEffort: z.string().nullable(),
		})
		.optional(),
	error: z.unknown().optional(),
});

/**
 * Window B of the reference (review F2): its `thread/resume` of window A's thread, on a connection
 * other than A's, and the server's reply. Pending until both are recorded; refuses at once an
 * error reply or a reply for another thread.
 */
export function observeReferenceAttach(
	recordingPath: string,
	threadId: string,
	windowAConnection: number,
): ReferenceAttachObservation {
	if (!existsSync(recordingPath)) return { kind: 'pending', reason: 'no wire recording yet' };
	const frames = parseWireRecording(readFileSync(recordingPath, 'utf8'));
	const resume = frames.flatMap((frame) => {
		if (frame.direction !== 'cliToServer' || frame.connection === windowAConnection) return [];
		const request = requestSchema.safeParse(frame.message);
		if (!request.success || request.data.method !== 'thread/resume') return [];
		const target = z
			.looseObject({ params: z.looseObject({ threadId: z.string() }) })
			.safeParse(frame.message);
		return target.success && target.data.params.threadId === threadId
			? [{ connection: frame.connection, ...request.data }]
			: [];
	})[0];
	if (resume === undefined)
		return { kind: 'pending', reason: `no thread/resume of ${threadId} from window B yet` };
	const reply = frames.flatMap((frame) => {
		if (frame.direction !== 'serverToCli' || frame.connection !== resume.connection) return [];
		const parsed = resumeReplySchema.safeParse(frame.message);
		return parsed.success && parsed.data.id === resume.id ? [parsed.data] : [];
	})[0];
	if (reply === undefined) return { kind: 'pending', reason: 'no thread/resume reply yet' };
	if (reply.result === undefined)
		throw new Error('the real-Codex reference window B has no successful thread/resume reply');
	if (reply.result.thread.id !== threadId)
		throw new Error(
			`the real-Codex reference window B resumed ${reply.result.thread.id}, not window A's ${threadId}`,
		);
	return {
		kind: 'attached',
		start: {
			connection: resume.connection,
			thread_id: reply.result.thread.id,
			model: reply.result.model,
			effort: reply.result.reasoningEffort,
			model_provider: reply.result.modelProvider,
		},
	};
}

/** Window B's launch readiness: its own successful resume of window A's thread is on the wire. */
export function referenceAttachReadiness(
	recordingPath: string,
	threadId: string,
	windowAConnection: number,
): { readonly ready: () => boolean; readonly pending: () => string } {
	return {
		ready: (): boolean =>
			observeReferenceAttach(recordingPath, threadId, windowAConnection).kind === 'attached',
		pending: (): string => {
			const observed = observeReferenceAttach(recordingPath, threadId, windowAConnection);
			return observed.kind === 'pending' ? observed.reason : 'attached';
		},
	};
}

/** Before window B sends any key: it attached to window A's thread with the declared model. */
export function requireReferenceAttach(
	recordingPath: string,
	threadId: string,
	windowAConnection: number,
	declared: ReferenceModel,
): ReferenceThreadStart {
	const observed = observeReferenceAttach(recordingPath, threadId, windowAConnection);
	if (observed.kind === 'pending')
		throw new Error(
			`the real-Codex reference window B did not attach (${observed.reason}); no input sent`,
		);
	return requireDeclared(observed.start, declared);
}
