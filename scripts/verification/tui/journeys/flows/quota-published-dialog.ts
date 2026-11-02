/** Collect real dialog paint using its live post-publication scroll witness. */
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

import type { InputConsumptionExpectation } from '../../sessions/input-consumption-schema.ts';
import type { DispatchFrameBinding } from '../../terminal/input-dispatch-frames.ts';
import type { MarkedInputEvidence } from '../../terminal/marker-input-session.ts';
import { findDialog } from '../scenes/grid-scene-matcher.ts';
import type { NativeFlowContext } from './native-flow-runtime.ts';
import { collectQuotaBodyRows, readQuotaBodyViewport } from './quota-body-scroll.ts';
import type { QuotaBodyViewport } from './quota-body-scroll.ts';
import { quotaExpectedKey, quotaMarkedFrame } from './quota-flow-inputs.ts';
import type { QuotaLiveFixture } from './quota-live-fixture.ts';

export async function collectPublishedQuotaDialog(options: {
	readonly context: NativeFlowContext;
	readonly title: string;
	readonly surface: InputConsumptionExpectation['surface'];
	readonly focusedTarget: string;
	readonly evidence: string;
	readonly fixture: Pick<QuotaLiveFixture, 'read'>;
}): Promise<{ readonly marked: MarkedInputEvidence; readonly rows: readonly string[] }> {
	const { context, title, surface, focusedTarget, evidence, fixture } = options;
	let marked = await context.input.press('F12', [quotaExpectedKey('F12', surface)]);
	const samples: {
		readonly binding: DispatchFrameBinding;
		readonly viewport: QuotaBodyViewport;
	}[] = [];
	const sample = async (): Promise<QuotaBodyViewport> => {
		const frame = quotaMarkedFrame(marked);
		const rectangle = findDialog(frame.grid, title);
		if (rectangle === null) throw new Error('published quota dialog absent: ' + title);
		const live = await fixture.read(frame.binding.sequence);
		const viewport = readQuotaBodyViewport({
			readback: live.readback,
			grid: frame.grid,
			rectangle,
			surface,
		});
		samples.push({ binding: frame.binding, viewport });
		return viewport;
	};
	const entrySequence = quotaMarkedFrame(marked).binding.sequence;
	const save = (complete: boolean, rows: readonly string[]): void => {
		writeFileSync(
			join(evidence, 'quota-body-census-' + entrySequence + '.json'),
			JSON.stringify({ title, surface, entrySequence, complete, samples, rows }, null, 2) + '\n',
		);
	};
	try {
		const initial = await sample();
		const rows = await collectQuotaBodyRows({
			initial,
			step: async (key) => {
				marked = await context.input.press(key, [quotaExpectedKey(key, surface, focusedTarget)]);
				return sample();
			},
		});
		save(true, rows);
		return { marked, rows };
	} catch (error) {
		save(false, []);
		throw error;
	}
}
