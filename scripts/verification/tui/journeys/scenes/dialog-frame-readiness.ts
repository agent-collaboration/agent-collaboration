/** Bind dialog targeting to its actual measurement paint and following redraw. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { InputConsumptionReadback } from '../../sessions/input-consumption-readback.ts';
import type { InputDispatchReceipt } from '../../sessions/input-consumption-schema.ts';
import type { KeyProtocol, PtyGridDriver } from '../../terminal/pty-grid-driver.ts';
import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';
import { encodeJourneyKey } from '../journey-key.ts';
import { findDialog } from './grid-scene-matcher.ts';

export async function openMeasuredDetails(
	driver: PtyGridDriver,
	directory: string,
	keys: KeyProtocol,
): Promise<void> {
	const before = driver.frameGeneration();
	const receipts = InputConsumptionReadback.open(
		join(directory, 'session-receipts', 'input-dispatch.jsonl'),
	);
	const sequence = receipts.sequence + 1;
	const [acknowledgement] = await receipts.observeInput({
		expected: [
			{
				event: {
					kind: 'key',
					code: { kind: 'fixed', name: 'Return' },
					modifiers: [],
					phase: 'press',
				},
				surface: { kind: 'screen' },
				target: 'sessions-list',
				action: 'details',
			},
		],
		inject: async () => {
			await driver.send(encodeJourneyKey('Enter', keys));
			await driver.waitForDispatchFrame(sequence);
		},
		timeoutMs: 8_000,
	});
	if (acknowledgement === undefined) throw new Error('Details input acknowledgement absent');
	await waitForMeasuredDialog({
		driver,
		directory,
		title: 'Session details',
		checkpoint: 'details',
		before,
		acknowledgement,
	});
}

export async function waitForMeasuredDialog(options: {
	readonly driver: Pick<
		PtyGridDriver,
		'frameGeneration' | 'bytes' | 'resizeHistory' | 'waitForGrid'
	>;
	readonly directory: string;
	readonly title: string;
	readonly checkpoint: string;
	readonly before: number;
	readonly acknowledgement: InputDispatchReceipt;
}): Promise<void> {
	const { driver, directory, title, checkpoint, before, acknowledgement } = options;
	const replay = await createTerminalCore(120, 36);
	let offset = 0;
	let resizeIndex = 0;
	let measurementFrame: number | null = null;
	const endMarker = Buffer.from('\x1b[?2026l');
	try {
		await driver.waitForGrid(
			`${title} after its measurement redraw`,
			(grid) => {
				const bytes = driver.bytes();
				for (
					let end = bytes.indexOf(endMarker, offset);
					end >= 0;
					end = bytes.indexOf(endMarker, offset)
				) {
					const next = end + endMarker.length;
					const resizes = driver.resizeHistory();
					for (
						let resize = resizes[resizeIndex];
						resize !== undefined && resize.byteOffset <= next;
						resize = resizes[resizeIndex]
					) {
						replay.writeRaw(bytes.subarray(offset, resize.byteOffset));
						offset = resize.byteOffset;
						replay.resize(resize.columns, resize.rows);
						resizeIndex += 1;
					}
					replay.writeRaw(bytes.subarray(offset, next));
					offset = next;
					const ordinal = replay.synchronizedOutputGeneration();
					if (
						measurementFrame === null &&
						ordinal > before &&
						findDialog(snapshotGrid(replay), title) !== null
					)
						measurementFrame = ordinal;
				}
				if (replay.synchronizedOutputGeneration() !== driver.frameGeneration())
					throw new Error('apparatus dialog frame ordinal differs from terminal parser');
				return (
					measurementFrame !== null &&
					driver.frameGeneration() > measurementFrame &&
					findDialog(grid, title) !== null
				);
			},
			before,
		);
		writeFileSync(
			join(directory, `${checkpoint}-frame-binding.json`),
			JSON.stringify(
				{
					kind: 'post-measurement-dialog',
					inputAcknowledgement: acknowledgement,
					beforeFrame: before,
					measurementFrame,
					minimumCompletedFrame: (measurementFrame ?? 0) + 1,
					receivedCompletedFrame: driver.frameGeneration(),
				},
				null,
				2,
			),
		);
	} finally {
		replay.dispose();
	}
}
