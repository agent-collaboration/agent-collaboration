/** A launch frame must authorize typing; only the known update modal gets one Escape. */
import type { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import type { KeyProtocol } from '../terminal/pty-grid-driver.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import {
	CODEX_0_165_TEXT,
	UI_CHECKPOINT_MS,
	encodeCodexAction,
	gridShowsEmptyComposer,
} from './codex-acp-scenes.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';

function showsUpdateModal(lines: readonly string[]): boolean {
	return (
		lines.some((line) => /Update available\s*·\s*0\.165\.0\s*→\s*\S+/u.test(line)) &&
		['1. Update now', '2. Skip', '3. Skip until next version', 'enter continue', 'esc skip'].every(
			(text) => lines.some((line) => line.includes(text)),
		)
	);
}

function composerReady(lines: readonly string[]): boolean {
	return (
		gridShowsEmptyComposer(lines) &&
		![
			'Update available',
			CODEX_0_165_TEXT.resumingSession,
			CODEX_0_165_TEXT.working,
			CODEX_0_165_TEXT.errorPrefix,
			CODEX_0_165_TEXT.modelPopupHeader,
		].some((text) => lines.some((line) => line.includes(text)))
	);
}

export async function requireLaunchComposer(inputs: {
	readonly driver: Pick<PtyGridDriver, 'frameGeneration' | 'send' | 'waitForGrid'>;
	readonly keys: KeyProtocol;
	readonly record: (step: CodexJourney['steps'][number], grid: TerminalGrid) => Promise<void>;
}): Promise<void> {
	const { driver, keys, record } = inputs;
	const capture = async (scene: string, grid: TerminalGrid): Promise<void> => {
		await record(
			{
				actions: [],
				checkpoint: {
					scene,
					require: [
						scene === 'startup-update-modal'
							? 'Update available'
							: scene === 'startup-composer-ready'
								? CODEX_0_165_TEXT.emptyComposerPlaceholder
								: (gridLines(grid).find((line) => line.trim() !== '') ?? 'no startup frame'),
					],
					forbid: [],
					deadline_ms: UI_CHECKPOINT_MS,
				},
			},
			grid,
		);
	};
	// Wait for a completed, nonempty frame rather than treating the PTY READY handshake as a
	// composer. Unknown frames are findings; they never trigger speculative keys.
	const first = await driver.waitForGrid(
		'startup-input-frame',
		(grid) => gridLines(grid).some((line) => line.trim() !== ''),
		-1,
		UI_CHECKPOINT_MS,
	);
	const lines = gridLines(first);
	if (showsUpdateModal(lines)) {
		await capture('startup-update-modal', first);
		const after = driver.frameGeneration();
		await driver.send(encodeCodexAction({ kind: 'key', key: 'Escape' }, keys));
		const dismissed = await driver.waitForGrid(
			'startup-after-escape',
			(grid) => gridLines(grid).some((line) => line.trim() !== ''),
			after,
			UI_CHECKPOINT_MS,
		);
		if (!composerReady(gridLines(dismissed))) {
			await capture('startup-composer-refused', dismissed);
			throw new Error('composer is not ready after one Escape; no further keys');
		}
		await capture('startup-composer-ready', dismissed);
		return;
	}
	if (!composerReady(lines)) {
		await capture('startup-composer-refused', first);
		throw new Error('composer is not ready; no keys sent');
	}
	await capture('startup-composer-ready', first);
}
