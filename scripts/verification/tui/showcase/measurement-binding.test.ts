import { expect, test } from 'vitest';

import { changedMainMeasurement, mainMeasurementFollowUp } from './measurement-binding.ts';

test('the public main ScrollPosition measurement, not a row-count guess, requires its follow-up draw', () => {
	const before = { mainScrollLayout: { naturalHeight: 36, viewportHeight: 32, viewportTop: 4 } };
	expect(changedMainMeasurement(before, before)).toBe(false);
	expect(
		changedMainMeasurement(before, {
			mainScrollLayout: { ...before.mainScrollLayout, naturalHeight: 2 },
		}),
	).toBe(true);
	expect(
		changedMainMeasurement(before, {
			mainScrollLayout: { ...before.mainScrollLayout, viewportHeight: 8 },
		}),
	).toBe(true);
	expect(
		changedMainMeasurement(before, {
			mainScrollLayout: { ...before.mainScrollLayout, viewportTop: 5 },
		}),
	).toBe(true);
	expect(changedMainMeasurement({ mainScrollLayout: null }, before)).toBe(false);
	expect(changedMainMeasurement(before, { mainScrollLayout: null })).toBe(false);
});
test('an intermediate burst measurement needs its own next draw, not an arbitrary draw after the last key', () => {
	const layout = { naturalHeight: 36, viewportHeight: 32, viewportTop: 4 };
	const frames = [
		{ sequence: 1, layout },
		{ sequence: 2, layout: { ...layout, naturalHeight: 35 } },
		{ sequence: 3, layout: { ...layout, naturalHeight: 2 } },
		{ sequence: 4, layout: { ...layout, naturalHeight: 2 } },
	];
	expect(mainMeasurementFollowUp(frames, 1)).toBe(4);
	expect(mainMeasurementFollowUp(frames, 3)).toBe(0);
	expect(mainMeasurementFollowUp(frames.slice(0, 3), 1)).toBe(4);
});
