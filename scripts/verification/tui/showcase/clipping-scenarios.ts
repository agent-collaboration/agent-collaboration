import { assertClippingPose } from './clipping-observations.ts';
import type { ClippingFixture } from './clipping-observations.ts';
/** Ordinary-View clipping, positive/negative pointer controls and same-child resize. */
import { PrerequisiteGap, requireObservation } from './contracts.ts';
import { expectedInput } from './input-expectations.ts';
import type { ShowcaseRig } from './rig.ts';
import { effectCount, escapeOverlay } from './scenario-helpers.ts';
async function assertInertClipPoint(
	rig: ShowcaseRig,
	point: { column: number; row: number },
	label: string,
): Promise<void> {
	if (
		point.column < 0 ||
		point.row < 0 ||
		point.column >= rig.grid().columns ||
		point.row >= rig.grid().rows
	)
		throw new PrerequisiteGap(
			'declared natural clipping coordinate is outside the physical viewport: ' + label,
		);
	await rig.burst({
		bytes: Buffer.from(
			'\x1b[<0;' +
				(point.column + 1) +
				';' +
				(point.row + 1) +
				'M\x1b[<0;' +
				(point.column + 1) +
				';' +
				(point.row + 1) +
				'm',
		),
		expected: [
			expectedInput(
				{ kind: 'mouse', mouse: { kind: 'down', button: 'left' }, ...point, modifiers: [] },
				rig.route(),
			),
			expectedInput(
				{ kind: 'mouse', mouse: { kind: 'up', button: 'left' }, ...point, modifiers: [] },
				rig.route(),
			),
		],
		redraw: false,
		label,
	});
}
async function observeClippingPose(
	rig: ShowcaseRig,
	fixture: ClippingFixture,
	expectedCalls: number,
): Promise<{ column: number; row: number }> {
	const { negativePoints } = assertClippingPose(rig.drawState(), rig.grid(), fixture);
	await rig.click('action:clip-hit', 'clip-hit');
	await rig.click('action:clip-partial', 'clip-partial');
	requireObservation(
		effectCount(rig.state(), 'clip-hit') === expectedCalls &&
			effectCount(rig.state(), 'clip-partial') === expectedCalls,
		'both visible positive controls activate once per pose',
		rig.state().effects,
	);
	await negativePoints.reduce<Promise<void>>(async (previous, point) => {
		await previous;
		await assertInertClipPoint(rig, point, 'hidden natural cells in ' + fixture);
	}, Promise.resolve());
	requireObservation(
		effectCount(rig.state(), 'clip-partial') === expectedCalls &&
			effectCount(rig.state(), 'clip-hidden') === 0,
		'hidden halves and fully hidden buttons remain inert on every ancestor intersection',
		rig.state().effects,
	);
	return rig.point('action:clip-hit');
}
export async function clippedGeometry(rig: ShowcaseRig, fixture: ClippingFixture): Promise<void> {
	await rig.menuAction('clip-' + fixture);
	const old = await observeClippingPose(rig, fixture, 1);
	await rig.capture('clipping-' + fixture + '-initial');
	await rig.resize(rig.grid().columns === 120 ? 76 : 120, rig.grid().rows === 36 ? 30 : 36);
	const current = rig.point('action:clip-hit');
	requireObservation(
		current.column !== old.column || current.row !== old.row,
		'the centered fixture actually moves in the same resized child',
		{ old, current },
	);
	await assertInertClipPoint(rig, old, 'old visible point after same-child resize');
	requireObservation(
		effectCount(rig.state(), 'clip-hit') === 1,
		'old coordinates do not activate the moved control',
		rig.state().effects,
	);
	await observeClippingPose(rig, fixture, 2);
	await rig.capture('clipping-' + fixture + '-resized');
	await escapeOverlay(rig);
}
