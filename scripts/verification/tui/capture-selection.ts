import { z } from 'zod';

import type { TerminalViewport } from './evidence-schema.ts';
import {
	captureTest,
	quotaCaptureTest,
	sessionsCaptureTest,
	sessionCheckpointStates,
	sessionDetailsPoses,
	quotaCheckpointStates,
	quotaNegativePairs,
	viewports,
} from './scene-contracts.ts';
export const captureSelectionSchema = z.enum([
	'static-both',
	'static-quota',
	'static-sessions',
	'cua-sessions',
	'cua-quota',
]);
export type CaptureSelection = z.infer<typeof captureSelectionSchema>;
export type Surface = 'sessions' | 'quota';
export function cuaSurface(selection: CaptureSelection): Surface | null {
	return selection === 'cua-quota' ? 'quota' : selection === 'cua-sessions' ? 'sessions' : null;
}
export function staticSurfaces(selection: CaptureSelection): readonly Surface[] {
	if (cuaSurface(selection) !== null)
		throw new Error('CUA selection has no static image collection');
	return selection === 'static-quota'
		? ['quota']
		: selection === 'static-sessions'
			? ['sessions']
			: ['sessions', 'quota'];
}
export function staticViewports(selection: CaptureSelection): readonly TerminalViewport[] {
	staticSurfaces(selection);
	return selection === 'static-sessions' ? [...viewports].toReversed() : viewports;
}
export function selectionTest(selection: CaptureSelection): string {
	return selection === 'static-quota'
		? quotaCaptureTest
		: selection === 'static-sessions'
			? sessionsCaptureTest
			: captureTest;
}
export function blankScene(selection: CaptureSelection): readonly [string, string] {
	return selection === 'static-quota' || selection === 'cua-quota'
		? ['compact-quota-all', 'Quota status']
		: ['compact-sessions-all', 'Sessions'];
}
export function captureArguments(
	driver: string,
	selection: CaptureSelection = 'static-both',
): string[] {
	return [
		driver,
		selectionTest(selection),
		'--exact',
		'--ignored',
		'--nocapture',
		'--test-threads=1',
	];
}
export function staticCaptureBudgetSeconds(selection: CaptureSelection): number {
	return (
		90 +
		staticSurfaces(selection).reduce(
			(total, surface) =>
				total +
				viewports.reduce(
					(sum, grid) =>
						sum +
						90 +
						40 +
						30 *
							(surface === 'sessions'
								? sessionCheckpointStates(grid).length +
									3 +
									sessionDetailsPoses(grid).length +
									(grid.columns === 48 && grid.rows === 12 ? 1 : 0)
								: quotaCheckpointStates(grid.rows).length + 3 + quotaNegativePairs.length),
					0,
				),
			0,
		)
	);
}
export function requireOneTest(discovery: string, testName: string = captureTest): void {
	const selected = discovery.split(/\r?\n/u).filter((line) => line.endsWith(': test'));
	if (selected.length !== 1 || selected[0] !== `${testName}: test`)
		throw new Error('exact capture test missing or broadened');
}
