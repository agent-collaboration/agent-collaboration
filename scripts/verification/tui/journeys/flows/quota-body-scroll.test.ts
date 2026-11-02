/** The body-scroll wire is live-only and must reject incomplete or impossible row facts. */
import { expect, test } from 'vitest';

import { collectQuotaBodyRows, readQuotaBodyViewport } from './quota-body-scroll.ts';
import type { QuotaBodyViewport } from './quota-body-scroll.ts';
import { quotaLivePoseSchema } from './quota-live-fixture.ts';

function livePose(bodyScroll: unknown): unknown {
	return {
		mount: 1,
		browseAccountId: 'effect-account-0',
		listSelection: 'effect-account-0',
		captured: null,
		rows: [],
		actions: [],
		scope: 'quota-account-actions',
		focus: null,
		published: { frame: 9, targets: [] },
		bodyScroll,
	};
}
const position = {
	surface: 'Overlay(OverlayId(1))',
	mount: 1,
	publicationFrame: 9,
	offset: 0,
	maxOffset: 3,
	viewportHeight: 1,
	contentHeight: 4,
};

test.each([
	['missing', undefined],
	['negative offset', { ...position, offset: -1 }],
	['beyond end', { ...position, offset: 4 }],
	['wrong maximum', { ...position, maxOffset: 2 }],
	['invisible viewport', { ...position, viewportHeight: 0 }],
	['negative content', { ...position, contentHeight: -1 }],
	['fractional row', { ...position, offset: 0.5 }],
	['missing mount', { ...position, mount: undefined }],
	['unknown wire field', { ...position, syntheticComplete: true }],
])('live body wire rejects %s', (_name, bodyScroll) => {
	// Arrange / Act: decode the real live boundary with a malformed new witness.
	const result = quotaLivePoseSchema.safeParse(livePose(bodyScroll));
	// Assert: silently stripping the witness must not turn bad state into readiness.
	expect(result.success).toBe(false);
});
test.each([
	null,
	position,
	{ ...position, offset: 3 },
	{ ...position, maxOffset: 0, contentHeight: 1 },
])('live body wire accepts the declared nullable and valid row states', (bodyScroll) => {
	expect(quotaLivePoseSchema.safeParse(livePose(bodyScroll)).success).toBe(true);
});

function bodySample(
	offset: number,
	frame: number,
	rows: readonly string[] = ['A', '', '', 'B'],
): QuotaBodyViewport {
	const row = rows[offset];
	if (row === undefined) throw new Error('test offset outside literal rows');
	return {
		position: {
			...position,
			offset,
			publicationFrame: frame,
			maxOffset: rows.length - 1,
			contentHeight: rows.length,
		},
		rows: [row],
		geometry: { left: 1, right: 10, top: 3, bottom: 3 },
	};
}
test.each([0, 2])(
	'one-row census keeps repeated blank rows and restores entry %s',
	async (entry) => {
		// Arrange: offset changes over identical paint, so row equality cannot mean end.
		let offset = entry,
			frame = 9;
		const keys: string[] = [];
		// Act: use the real census, replacing only the product's published boundary.
		const rows = await collectQuotaBodyRows({
			initial: bodySample(offset, frame),
			step: async (key) => {
				keys.push(key);
				offset += key === 'Down' ? 1 : -1;
				frame += 1;
				return bodySample(offset, frame);
			},
		});
		// Assert: no skipped blank row, complete body, equal input counts and exact entry restoration.
		expect(rows).toEqual(['A', '', '', 'B']);
		expect(keys.filter((key) => key === 'Down')).toHaveLength(3);
		expect(keys.filter((key) => key === 'Up')).toHaveLength(3);
		expect(offset).toBe(entry);
	},
);
test('fitting one-row body completes without inventing scroll input', async () => {
	const rows = await collectQuotaBodyRows({
		initial: bodySample(0, 9, ['A']),
		step: async () => {
			throw new Error('fitting body must not scroll');
		},
	});
	expect(rows).toEqual(['A']);
});
test.each(['stale-frame', 'wrong-mount', 'skipped-offset', 'changed-geometry'] as const)(
	'body census rejects %s',
	async (kind) => {
		const initial = bodySample(0, 9);
		const next = bodySample(1, 10);
		if (kind === 'stale-frame') next.position.publicationFrame = 9;
		if (kind === 'wrong-mount') next.position.mount = 2;
		if (kind === 'skipped-offset') next.position.offset = 2;
		const broken =
			kind === 'changed-geometry' ? { ...next, geometry: { ...next.geometry, top: 4 } } : next;
		await expect(collectQuotaBodyRows({ initial, step: async () => broken })).rejects.toThrow(
			'fresh exact one-row offset progression',
		);
	},
);
test('body census rejects wrong restored paint even when offset is correct', async () => {
	let offset = 0,
		frame = 9;
	await expect(
		collectQuotaBodyRows({
			initial: bodySample(0, frame, ['A', 'B']),
			step: async (key) => {
				offset += key === 'Down' ? 1 : -1;
				frame += 1;
				return bodySample(offset, frame, key === 'Up' ? ['wrong', 'B'] : ['A', 'B']);
			},
		}),
	).rejects.toThrow('restoration differs');
});

import { createTerminalCore, snapshotGrid } from '../../terminal/terminal-grid.ts';

test.each([
	'valid',
	'fitting',
	'null',
	'wrong-surface',
	'wrong-mount',
	'stale-frame',
	'height-mismatch',
	'missing-area',
	'broken-cells',
] as const)('published body paint binding: %s', async (kind) => {
	// Arrange: real emulator cells plus an independently declared published ScrollArea.
	const core = await createTerminalCore(14, 10);
	try {
		core.writeString('\x1b[3;2HA');
		const geometry = Array.from({ length: 10 }, (_, index) => [index + 1, 2]);
		const bodyScroll =
			kind === 'null'
				? null
				: {
						...position,
						...(kind === 'fitting' ? { maxOffset: 0, contentHeight: 1 } : {}),
						...(kind === 'wrong-surface' ? { surface: 'Overlay(OverlayId(2))' } : {}),
						...(kind === 'wrong-mount' ? { mount: 2 } : {}),
						...(kind === 'height-mismatch' ? { viewportHeight: 2, maxOffset: 2 } : {}),
					};
		const readback = {
			...quotaLivePoseSchema.parse(livePose(bodyScroll)),
			published: {
				frame: kind === 'stale-frame' ? 10 : 9,
				targets:
					kind === 'missing-area'
						? []
						: [
								{
									surface: 'Overlay(OverlayId(1))',
									target: 'action:quota:dialog-body',
									kind: 'ScrollArea',
									cells:
										kind === 'broken-cells' ? geometry.filter((_, index) => index !== 3) : geometry,
								},
							],
			},
		};
		// Act: correlate the live witness and region with the active marked paint.
		const read = (): QuotaBodyViewport =>
			readQuotaBodyViewport({
				readback,
				grid: snapshotGrid(core),
				rectangle: { left: 0, right: 12, top: 0, bottom: 8 },
				surface: { kind: 'overlay', overlayId: '1' },
			});
		// Assert: fitting and one-row states work; foreign/stale/covered/missing geometry never supplies evidence.
		if (kind === 'valid' || kind === 'fitting') expect(read().rows).toEqual(['A']);
		else expect(read).toThrow();
	} finally {
		core.dispose();
	}
});
test('multi-row census validates overlapping paint and keeps blank rows', async () => {
	const full = ['A', '', '', 'B'];
	let offset = 0,
		frame = 9;
	const sampleViewport = (): QuotaBodyViewport => ({
		position: { ...position, offset, publicationFrame: frame, maxOffset: 2, viewportHeight: 2 },
		rows: full.slice(offset, offset + 2),
		geometry: { left: 1, right: 10, top: 3, bottom: 4 },
	});
	const rows = await collectQuotaBodyRows({
		initial: sampleViewport(),
		step: async (key) => {
			offset += key === 'Down' ? 1 : -1;
			frame += 1;
			return sampleViewport();
		},
	});
	expect(rows).toEqual(['A', '', '', 'B']);
	expect(offset).toBe(0);
});
test('multi-row census rejects a skipped painted row despite correct offset', async () => {
	const initial: QuotaBodyViewport = {
		...bodySample(0, 9),
		position: { ...position, viewportHeight: 2, maxOffset: 2 },
		rows: ['A', 'B'],
		geometry: { left: 1, right: 10, top: 3, bottom: 4 },
	};
	const next = {
		...initial,
		position: { ...initial.position, offset: 1, publicationFrame: 10 },
		rows: ['wrong', 'C'],
	};
	await expect(collectQuotaBodyRows({ initial, step: async () => next })).rejects.toThrow(
		'paint did not advance',
	);
});
