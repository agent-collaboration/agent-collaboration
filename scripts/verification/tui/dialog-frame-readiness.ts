/** Bind dialog targeting to its actual measurement paint and following redraw. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { findDialog } from './grid-scene-matcher.ts';
import { InputConsumptionReadback } from './input-consumption-readback.ts';
import { encodeJourneyKey } from './journey-key.ts';
import type { KeyProtocol, PtyGridDriver } from './pty-grid-driver.ts';
import { createTerminalCore, snapshotGrid } from './terminal-grid.ts';

export async function openMeasuredDetails(
	driver: PtyGridDriver,
	directory: string,
	keys: KeyProtocol,
): Promise<void> {
	const before = driver.frameGeneration();
	const receipts = InputConsumptionReadback.open(
		join(directory, 'session-receipts', 'input-dispatch.jsonl'),
	);
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
		inject: () => driver.send(encodeJourneyKey('Enter', keys)),
		timeoutMs: 8_000,
	});
	const replay = await createTerminalCore(120, 36);
	let offset = 0;
	let measurementFrame: number | null = null;
	const endMarker = Buffer.from('\x1b[?2026l');
	try {
		await driver.waitForGrid(
			'Details after its measurement redraw',
			(grid) => {
				const bytes = driver.bytes();
				for (
					let end = bytes.indexOf(endMarker, offset);
					end >= 0;
					end = bytes.indexOf(endMarker, offset)
				) {
					const next = end + endMarker.length;
					replay.writeRaw(bytes.subarray(offset, next));
					offset = next;
					const ordinal = replay.synchronizedOutputGeneration();
					if (
						measurementFrame === null &&
						ordinal > before &&
						findDialog(snapshotGrid(replay), 'Session details') !== null
					)
						measurementFrame = ordinal;
				}
				if (replay.synchronizedOutputGeneration() !== driver.frameGeneration())
					throw new Error('apparatus dialog frame ordinal differs from terminal parser');
				return (
					measurementFrame !== null &&
					driver.frameGeneration() > measurementFrame &&
					findDialog(grid, 'Session details') !== null
				);
			},
			before,
		);
		writeFileSync(
			join(directory, 'details-frame-binding.json'),
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
