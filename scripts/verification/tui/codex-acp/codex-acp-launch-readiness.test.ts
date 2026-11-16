/**
 * Launch readiness for the real-Codex reference (assignment Correction 30 and its addendum): real
 * Codex 0.166.0 draws a transient composer, then its update modal, and starts its thread only
 * after the modal is dismissed. Controlled frames drawn by the real terminal core and a scripted
 * frame source; the recording is fixture data. Never a live run; no real time passes.
 */
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { requireLaunchComposer } from './codex-acp-composer-ready.ts';
import { referenceLaunchReadiness } from './codex-acp-reference-start.ts';
import { LAUNCHED_THREAD, referenceStartRecording } from './codex-acp-test-fixtures.ts';

const COMPOSER = ['› Ask Codex to do anything', '  gpt-6-luna low · 1M window'];
// The literal 0.166.0 update modal from the D0 r1 c29 run (out-discovery/terminal-grid.json).
const UPDATE = [
	'Update available · 0.166.0 → 0.169.0',
	'Release notes: https://github.com/openai/codex/releases/latest',
	'› 1. Update now (runs the installer)',
	'  2. Skip',
	'  3. Skip until next version',
	'enter continue · esc skip',
];
const UNKNOWN_DIALOG = ['  Enable memories?', '  Yes, enable', '  Not now'];

async function drawn(lines: readonly string[]): Promise<TerminalGrid> {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString(lines.map((line, row) => `\x1b[${row + 1};1H${line}`).join(''));
		const composer = lines.findIndex((line) => line.startsWith('› Ask Codex'));
		core.writeString(composer >= 0 ? `\x1b[${composer + 1};3H\x1b[?25h` : '\x1b[?25l');
		return snapshotGrid(core);
	} finally {
		core.dispose();
	}
}

interface ScriptedFrame {
	readonly grid: TerminalGrid;
	// What reaches the wire recording before this frame is drawn.
	readonly beforeDraw?: () => void;
}

/**
 * A terminal that draws `segments[0]`, then one more segment after each key it receives. When no
 * frame remains, each wire event changes the recording with **no** redraw and the wait judges the
 * current grid again, as the tee's wake does (review F9). A wait nothing satisfies ends at the bound.
 */
function scriptedLaunch(
	segments: readonly (readonly ScriptedFrame[])[],
	wireEvents: readonly (() => void)[] = [],
): {
	readonly driver: Parameters<typeof requireLaunchComposer>[0]['driver'];
	readonly keys: () => string[];
} {
	const keys: string[] = [];
	let generation = 0;
	let unlocked = 1;
	const pending: ScriptedFrame[] = [...(segments[0] ?? [])];
	const events = [...wireEvents];
	let current: TerminalGrid | null = null;
	return {
		driver: {
			frameGeneration: (): number => generation,
			send: async (bytes: Uint8Array): Promise<void> => {
				keys.push(Buffer.from(bytes).toString());
				pending.push(...(segments[unlocked] ?? []));
				unlocked += 1;
			},
			waitForGrid: async (name, matches): Promise<TerminalGrid> => {
				for (let next = pending.shift(); next !== undefined; next = pending.shift()) {
					next.beforeDraw?.();
					generation += 1;
					current = next.grid;
					if (matches(next.grid)) return next.grid;
				}
				for (let event = events.shift(); event !== undefined; event = events.shift()) {
					event();
					if (current !== null && matches(current)) return current;
				}
				throw new Error(`grid checkpoint unreachable: ${name}`);
			},
		},
		keys: (): string[] => keys,
	};
}

const directories: string[] = [];
afterEach(() => {
	for (const directory of directories.splice(0))
		rmSync(directory, { recursive: true, force: true });
});
function recordingPath(): string {
	const directory = mkdtempSync(join(tmpdir(), 'launch-readiness-'));
	directories.push(directory);
	return join(directory, 'wire.jsonl');
}

const START = referenceStartRecording(LAUNCHED_THREAD);
const REQUEST_ONLY = `${START.trim().split('\n').slice(0, 3).join('\n')}\n`;

async function launch(
	segments: readonly (readonly ScriptedFrame[])[],
	path: string | null,
): Promise<{ keys: string[]; scenes: string[] }> {
	const { driver, keys } = scriptedLaunch(segments);
	const scenes: string[] = [];
	await requireLaunchComposer({
		driver,
		keys: 'legacy',
		record: async (step): Promise<void> => {
			scenes.push(step.checkpoint.scene);
		},
		launchCwd: '/Users/me/code/projects/codex-router.d0-runs',
		boundMs: 60_000,
		...(path === null ? {} : { launchReadiness: referenceLaunchReadiness(path) }),
	});
	return { keys: keys(), scenes };
}

test('composer, then the update modal, then thread/start: one Escape, ready only after the reply', async () => {
	const path = recordingPath();
	const composer = await drawn(COMPOSER);
	const result = await launch(
		[
			[{ grid: composer }, { grid: await drawn(UPDATE) }],
			[
				{ grid: composer, beforeDraw: (): void => writeFileSync(path, REQUEST_ONLY) },
				{ grid: composer, beforeDraw: (): void => writeFileSync(path, START) },
			],
		],
		path,
	);
	expect(result.keys).toEqual(['\x1b']);
	expect(result.scenes).toEqual(['startup-update-modal', 'startup-composer-ready']);
});

test('without a declared readiness the transient composer is taken at once (the c29 stop)', async () => {
	const composer = await drawn(COMPOSER);
	const result = await launch([[{ grid: composer }, { grid: await drawn(UPDATE) }]], null);
	expect(result.keys).toEqual([]);
	expect(result.scenes).toEqual(['startup-composer-ready']);
});

test('a reply that never arrives refuses at the startup bound, after the one Escape and no input', async () => {
	const path = recordingPath();
	const composer = await drawn(COMPOSER);
	const { driver, keys } = scriptedLaunch([
		[{ grid: composer }, { grid: await drawn(UPDATE) }],
		[{ grid: composer, beforeDraw: (): void => writeFileSync(path, REQUEST_ONLY) }],
	]);
	await expect(
		requireLaunchComposer({
			driver,
			keys: 'legacy',
			record: async (): Promise<void> => undefined,
			launchCwd: '/Users/me/code/projects/codex-router.d0-runs',
			boundMs: 60_000,
			launchReadiness: referenceLaunchReadiness(path),
		}),
	).rejects.toThrow(
		'the launch did not reach its declared readiness within its startup bound (no thread/start reply yet); no input sent',
	);
	expect(keys()).toEqual(['\x1b']);
});

test('an unknown dialog while waiting still stops with no key', async () => {
	const path = recordingPath();
	const { driver, keys } = scriptedLaunch([
		[{ grid: await drawn(COMPOSER) }, { grid: await drawn(UNKNOWN_DIALOG) }],
	]);
	await expect(
		requireLaunchComposer({
			driver,
			keys: 'legacy',
			record: async (): Promise<void> => undefined,
			launchCwd: '/Users/me/code/projects/codex-router.d0-runs',
			boundMs: 60_000,
			launchReadiness: referenceLaunchReadiness(path),
		}),
	).rejects.toThrow('composer is not ready; no keys sent');
	expect(keys()).toEqual([]);
});

test('a resume seen while waiting refuses at once', async () => {
	const path = recordingPath();
	const composer = await drawn(COMPOSER);
	const resumed = START.replace('"thread/start"', '"thread/resume"');
	const { driver } = scriptedLaunch([
		[{ grid: composer }, { grid: composer, beforeDraw: (): void => writeFileSync(path, resumed) }],
	]);
	await expect(
		requireLaunchComposer({
			driver,
			keys: 'legacy',
			record: async (): Promise<void> => undefined,
			launchCwd: '/Users/me/code/projects/codex-router.d0-runs',
			boundMs: 60_000,
			launchReadiness: referenceLaunchReadiness(path),
		}),
	).rejects.toThrow('resumed a thread instead of starting one');
});

test('a reply that reaches the wire after the last redraw is seen without another frame (F9)', async () => {
	const path = recordingPath();
	const composer = await drawn(COMPOSER);
	const { driver, keys } = scriptedLaunch(
		[
			[{ grid: composer }, { grid: await drawn(UPDATE) }],
			[{ grid: composer, beforeDraw: (): void => writeFileSync(path, REQUEST_ONLY) }],
		],
		[(): void => writeFileSync(path, START)],
	);
	await requireLaunchComposer({
		driver,
		keys: 'legacy',
		record: async (): Promise<void> => undefined,
		launchCwd: '/Users/me/code/projects/codex-router.d0-runs',
		boundMs: 60_000,
		launchReadiness: referenceLaunchReadiness(path),
	});
	expect(keys()).toEqual(['\x1b']);
});

test('an error reply that reaches the wire with no redraw refuses at once (F9)', async () => {
	const path = recordingPath();
	const composer = await drawn(COMPOSER);
	const errorReply = `${REQUEST_ONLY}${JSON.stringify({
		seq: 3,
		connection: 0,
		direction: 'serverToCli',
		message: { ['id']: 7, error: { code: -32600, message: 'refused' } },
	})}\n`;
	const later: string[] = [];
	const { driver } = scriptedLaunch(
		[[{ grid: composer, beforeDraw: (): void => writeFileSync(path, REQUEST_ONLY) }]],
		[
			(): void => writeFileSync(path, errorReply),
			// Anything after the refusal must never be reached.
			(): void => {
				later.push('reached');
			},
		],
	);
	await expect(
		requireLaunchComposer({
			driver,
			keys: 'legacy',
			record: async (): Promise<void> => undefined,
			launchCwd: '/Users/me/code/projects/codex-router.d0-runs',
			boundMs: 60_000,
			launchReadiness: referenceLaunchReadiness(path),
		}),
	).rejects.toThrow('no successful thread/start reply');
	expect(later).toEqual([]);
});
