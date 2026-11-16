/**
 * When a step's checkpoint may be judged. The PTY driver applies output as it arrives, so the
 * frame a step needs can already be on screen when the step starts (D0 r2: the status footer
 * redrew to "Haiku 5.5 low" while the previous step's frame was being recorded). Controlled
 * frames drawn by the real terminal core; never a live run.
 */
import { expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { driveCodexStep } from './codex-acp-run.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';

async function drawn(lines: readonly string[]): Promise<TerminalGrid> {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString(lines.map((line, row) => `\x1b[${row + 1};1H${line}`).join(''));
		const composer = lines.findIndex((line) => line.startsWith('› Ask Codex'));
		core.writeString(`\x1b[${composer + 1};3H`);
		return snapshotGrid(core);
	} finally {
		core.dispose();
	}
}

// The r2 frames: model-applied (frame 770) still shows high; frame 771 shows the applied low.
const APPLIED_HIGH = [
	'• Model changed to haiku low for this session only',
	'',
	'› Ask Codex to do anything',
	'',
	'  Haiku 5.5 high · 1M window · ← for agents',
];
const APPLIED_LOW = APPLIED_HIGH.map((line) => line.replace('high ·', 'low ·'));

const FOOTER: CodexJourney['steps'][number] = {
	actions: [],
	checkpoint: {
		scene: 'status-footer',
		require: ['Haiku 5.5 low', '1M window'],
		forbid: ['■ '],
		deadline_ms: 8_000,
		empty_composer: true,
	},
};

/** A terminal whose current frame (generation `generation`) is `grid`; no later frame arrives. */
function settledDriver(
	grid: TerminalGrid,
	generation: number,
): Parameters<typeof driveCodexStep>[0] & { sent: () => number } {
	let keys = 0;
	return {
		frameGeneration: (): number => generation,
		send: async (): Promise<void> => {
			keys += 1;
		},
		waitForGrid: async (name, matches, after): Promise<TerminalGrid> => {
			if (generation > (after ?? -1) && matches(grid)) return grid;
			throw new Error(`grid checkpoint unreachable: ${name}`);
		},
		sent: (): number => keys,
	};
}

test('an action-less step is judged on the frame already on screen', async () => {
	// Frame 771 arrived while the previous step was recorded; nothing newer will be drawn.
	const driver = settledDriver(await drawn(APPLIED_LOW), 771);
	await expect(driveCodexStep(driver, FOOTER, 'legacy')).resolves.toBeDefined();
	expect(driver.sent()).toBe(0);
});

test('an action-less step still fails when the frame on screen does not show its checkpoint', async () => {
	const driver = settledDriver(await drawn(APPLIED_HIGH), 770);
	await expect(driveCodexStep(driver, FOOTER, 'legacy')).rejects.toThrow(
		'grid checkpoint unreachable: status-footer',
	);
});

test('a step with actions still needs a frame drawn after its keys', async () => {
	// The matching frame predates the key and the terminal draws nothing after it.
	const driver = settledDriver(await drawn(APPLIED_LOW), 771);
	const typed: CodexJourney['steps'][number] = {
		...FOOTER,
		actions: [{ kind: 'key', key: 'Enter' }],
		checkpoint: { ...FOOTER.checkpoint, scene: 'after-enter' },
	};
	await expect(driveCodexStep(driver, typed, 'legacy')).rejects.toThrow(
		'grid checkpoint unreachable: after-enter',
	);
	expect(driver.sent()).toBe(1);
});
