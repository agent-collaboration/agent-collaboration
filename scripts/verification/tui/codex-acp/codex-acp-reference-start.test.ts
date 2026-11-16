/**
 * The real-Codex reference's model proof over recorded tee frames (assignment Corrections 29, 30):
 * fixture recordings shaped like Codex's thread/start exchange, and a scripted frame source in
 * place of the driver's bounded frame wait. Never a live run; no real time passes.
 */
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import {
	REFERENCE_THREAD_START_FILE,
	awaitReferenceThreadStart,
	observeReferenceAttach,
	referenceModelProof,
	requireReferenceAttach,
	requireReferenceThreadStart,
} from './codex-acp-reference-start.ts';
import {
	LAUNCHED_THREAD,
	REFERENCE_MODEL,
	referenceStartRecording,
} from './codex-acp-test-fixtures.ts';

const directories: string[] = [];
afterEach(() => {
	for (const directory of directories.splice(0))
		rmSync(directory, { recursive: true, force: true });
});
function scratch(): string {
	const directory = mkdtempSync(join(tmpdir(), 'reference-start-'));
	directories.push(directory);
	return directory;
}
function recording(text: string): string {
	const path = join(scratch(), 'wire.jsonl');
	writeFileSync(path, text);
	return path;
}

const START = referenceStartRecording(LAUNCHED_THREAD);
// The start request alone: the CLI has asked, the reply has not reached the tee yet.
const REQUEST_ONLY = `${START.trim().split('\n').slice(0, 3).join('\n')}\n`;

/**
 * A stand-in for the driver's bounded frame wait: before each scripted frame its change is applied
 * to the recording, then the readiness check runs. When no frame matches, the bound is reached.
 */
function scriptedFrames(
	changes: readonly (() => void)[],
): (matches: () => boolean, boundMs: number) => Promise<void> {
	return async (matches, boundMs): Promise<void> => {
		expect(boundMs).toBe(60_000);
		for (const change of changes) {
			change();
			if (matches()) return;
		}
		throw new Error('grid checkpoint unreachable: reference-thread-start');
	};
}

function awaitStart(
	recordingPath: string,
	changes: readonly (() => void)[],
): ReturnType<typeof awaitReferenceThreadStart> {
	return awaitReferenceThreadStart({
		recordingPath,
		declared: REFERENCE_MODEL,
		boundMs: 60_000,
		waitForFrames: scriptedFrames(changes),
	});
}

test('(a) a reply that arrives after the first look is waited for, then proved', async () => {
	const path = recording(REQUEST_ONLY);
	const started = await awaitStart(path, [
		(): void => undefined,
		(): void => writeFileSync(path, START),
	]);
	expect(started).toEqual({
		connection: 0,
		thread_id: LAUNCHED_THREAD,
		model: 'gpt-6-luna',
		effort: 'low',
		model_provider: 'codex-router-debug',
	});
});

test('(b) a reply that never arrives is refused at the startup bound, with no input', async () => {
	const path = recording(REQUEST_ONLY);
	await expect(awaitStart(path, [(): void => undefined])).rejects.toThrow(
		'reported no successful thread/start within its startup bound (no thread/start reply yet); no input sent',
	);
});

test('(c) a recording file that appears late is waited for, then proved', async () => {
	const path = join(scratch(), 'wire.jsonl');
	const started = await awaitStart(path, [
		(): void => writeFileSync(path, REQUEST_ONLY),
		(): void => writeFileSync(path, START),
	]);
	expect(started.thread_id).toBe(LAUNCHED_THREAD);
});

test('a start already on the wire is proved without waiting for a frame', async () => {
	const path = recording(START);
	const started = await awaitReferenceThreadStart({
		recordingPath: path,
		declared: REFERENCE_MODEL,
		boundMs: 60_000,
		waitForFrames: () => Promise.reject(new Error('no frame wait expected')),
	});
	expect(started.thread_id).toBe(LAUNCHED_THREAD);
});

test.each([
	['another model', { model: 'gpt-6.1-sol' }],
	['another effort', { effort: 'high' }],
	['no effort', { effort: null }],
	['another provider', { provider: 'openai' }],
] as const)('(d) a start reporting %s is refused before input', async (_name, reply) => {
	const late = recording(REQUEST_ONLY);
	const wrong = referenceStartRecording(LAUNCHED_THREAD, reply);
	await expect(awaitStart(late, [(): void => writeFileSync(late, wrong)])).rejects.toThrow(
		'no input sent',
	);
	expect(() => requireReferenceThreadStart(recording(wrong), REFERENCE_MODEL)).toThrow(
		'no input sent',
	);
});

const RESUMED = START.replace('"thread/start"', '"thread/resume"');
const ERROR_REPLY = `${REQUEST_ONLY}${JSON.stringify({
	seq: 3,
	connection: 0,
	direction: 'serverToCli',
	message: { ['id']: 7, error: { code: -32600, message: 'refused' } },
})}\n`;
const TWICE = START + START.replaceAll('"seq":', '"seq":1').replaceAll('"id":7', '"id":8');

test.each([
	['a resume', RESUMED, 'resumed a thread instead of starting one'],
	['an error reply', ERROR_REPLY, 'no successful thread/start reply'],
	['a second start', TWICE, 'started more than one thread'],
] as const)('(d) %s is refused at once, mid-wait too', async (_name, text, message) => {
	expect(() => requireReferenceThreadStart(recording(text), REFERENCE_MODEL)).toThrow(message);
	const late = recording(REQUEST_ONLY);
	await expect(awaitStart(late, [(): void => writeFileSync(late, text)])).rejects.toThrow(message);
});

test('the run hook waits, proves and keeps the reported start', async () => {
	const directory = scratch();
	const path = join(directory, 'wire.jsonl');
	await referenceModelProof({
		recordingPath: path,
		declared: REFERENCE_MODEL,
		output: directory,
		boundMs: 60_000,
	})(scriptedFrames([(): void => writeFileSync(path, START)]));
	expect(
		JSON.parse(readFileSync(join(directory, REFERENCE_THREAD_START_FILE), 'utf8')),
	).toMatchObject({ thread_id: LAUNCHED_THREAD, model: 'gpt-6-luna' });
});

test('a scripted /resume after the reference started is a journey action, not a refusal (F3)', () => {
	const pick = [
		{
			seq: 4,
			connection: 0,
			direction: 'cliToServer',
			message: { ['id']: 9, method: 'thread/resume', params: { threadId: 'other-thread' } },
		},
		{
			seq: 5,
			connection: 0,
			direction: 'serverToCli',
			message: { ['id']: 9, result: { thread: { ['id']: 'other-thread' } } },
		},
	]
		.map((frame) => JSON.stringify(frame))
		.join('\n');
	const started = requireReferenceThreadStart(recording(`${START}${pick}\n`), REFERENCE_MODEL);
	expect(started.thread_id).toBe(LAUNCHED_THREAD);
	// The same resume before the reference's own start is still refused.
	expect(() =>
		requireReferenceThreadStart(recording(`${pick}\n${START}`), REFERENCE_MODEL),
	).toThrow('resumed a thread instead of starting one');
});

/** Window B (connection 1) resumes window A's thread; the reply as Codex shapes ThreadResumeResponse. */
function attachFrames(
	reply: 'ok' | 'none' | 'error' | 'other' | 'luna-high',
	connection = 1,
): string {
	const frames: unknown[] = [
		{
			seq: 4,
			connection,
			direction: 'cliToServer',
			message: { ['id']: 3, method: 'thread/resume', params: { threadId: LAUNCHED_THREAD } },
		},
	];
	if (reply !== 'none')
		frames.push({
			seq: 5,
			connection,
			direction: 'serverToCli',
			message:
				reply === 'error'
					? { ['id']: 3, error: { code: -32600, message: 'refused' } }
					: {
							['id']: 3,
							result: {
								thread: { ['id']: reply === 'other' ? 'another-thread' : LAUNCHED_THREAD },
								model: 'gpt-6-luna',
								modelProvider: 'codex-router-debug',
								reasoningEffort: reply === 'luna-high' ? 'high' : 'low',
							},
						},
		});
	return `${frames.map((frame) => JSON.stringify(frame)).join('\n')}\n`;
}

test('window B attaches to window A thread and is proved from its own resume reply (F2)', () => {
	const path = recording(START + attachFrames('ok'));
	expect(observeReferenceAttach(path, LAUNCHED_THREAD, 0).kind).toBe('attached');
	expect(requireReferenceAttach(path, LAUNCHED_THREAD, 0, REFERENCE_MODEL)).toMatchObject({
		connection: 1,
		thread_id: LAUNCHED_THREAD,
		effort: 'low',
	});
});

test('window B is pending until its resume and reply are recorded; A resuming does not count', () => {
	expect(observeReferenceAttach(recording(START), LAUNCHED_THREAD, 0).kind).toBe('pending');
	expect(
		observeReferenceAttach(recording(START + attachFrames('none')), LAUNCHED_THREAD, 0).kind,
	).toBe('pending');
	expect(
		observeReferenceAttach(recording(START + attachFrames('ok', 0)), LAUNCHED_THREAD, 0).kind,
	).toBe('pending');
	expect(() =>
		requireReferenceAttach(recording(START), LAUNCHED_THREAD, 0, REFERENCE_MODEL),
	).toThrow('did not attach');
});

test.each([
	['an error reply', 'error', 'no successful thread/resume reply'],
	['a reply for another thread', 'other', 'resumed another-thread, not window A'],
	['another effort', 'luna-high', 'no input sent'],
] as const)('window B with %s is refused before any key (F2)', (_name, reply, message) => {
	const path = recording(START + attachFrames(reply));
	expect(() => requireReferenceAttach(path, LAUNCHED_THREAD, 0, REFERENCE_MODEL)).toThrow(message);
});

/** A thread/start the CLI sends after the launch, with the given threadSource. */
const laterStart = (source: string): string =>
	`${JSON.stringify({
		seq: 4,
		connection: 0,
		direction: 'cliToServer',
		message: { ['id']: 'temp-1', method: 'thread/start', params: { threadSource: source } },
	})}\n`;

test("Codex's own title thread after the launch is not a second start (Correction 33)", () => {
	expect(
		requireReferenceThreadStart(recording(START + laterStart('thread_title')), REFERENCE_MODEL)
			.thread_id,
	).toBe(LAUNCHED_THREAD);
	// Any other extra start is still a second thread.
	expect(() =>
		requireReferenceThreadStart(recording(START + laterStart('user')), REFERENCE_MODEL),
	).toThrow('started more than one thread');
});
