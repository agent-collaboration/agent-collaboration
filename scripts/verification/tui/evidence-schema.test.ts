import { describe, expect, test } from 'vitest';

import {
	captureArguments,
	staticSurfaces,
	staticViewports,
	blankScene,
	staticCaptureBudgetSeconds,
} from './capture-selection.ts';
import { requireSamePty } from './cua-readback.ts';
import {
	artifactBindingSchema,
	capturePlanSchema,
	choiceFocusSchema,
	cuaReadbackSchema,
	cuaReadySchema,
	filterReceiptSchema,
	imageObservationSchema,
	imageRejectionSchema,
	requireSelectedChoice,
} from './evidence-schema.ts';
import { checkpointDialog, coverageGaps } from './scene-contracts.ts';

function receiptLiteral(): Record<string, unknown> {
	return {
		surface: 'sessions',
		columns: 120,
		rows: 36,
		node_sha256: '0'.repeat(64),
		binary_sha256: 'a'.repeat(64),
		vhs_sha256: 'b'.repeat(64),
		ttyd_sha256: 'c'.repeat(64),
		verifier_sha256: 'd'.repeat(64),
		verifier_source_sha256: 'e'.repeat(64),
		verifier_checkpoint_source_sha256: 'c'.repeat(64),
		driver_source_sha256: 'f'.repeat(64),
		tape_driver_source_sha256: '0'.repeat(64),
		fixed_child_sha256: '1'.repeat(64),
		tape_sha256: '2'.repeat(64),
		state_and_secrets_unchanged: true,
		provider_requests: 0,
		child_exit: 0,
		owned_processes_stopped: true,
		fixture: 'protocol only',
		images: ['all', 'chooser', 'help', 'claude', 'cancelled', 'details'].map((state) => ({
			checkpoint: `compact-sessions-${state}`,
			png_sha256: '3'.repeat(64),
		})),
	};
}
function focusObservation(measurements: readonly unknown[]): unknown {
	return {
		checkpoint: 'compact-quota-hidden-choice',
		recognizedLines: ['Account filters', 'Shown', 'Hidden', 'Cancel Apply'],
		recognizedFieldPairs: [],
		choiceFocusMeasurements: measurements,
		missingMarkers: [],
		unexpectedMarkers: [],
	};
}
const originalFocus = {
	label: 'hidden',
	labelYellowPixels: 131,
	glyphYellowPixels: 25,
	glyphColumns: 5,
	glyphRows: 12,
	labelHeight: 20,
};
describe('strict evidence contracts', () => {
	test('sessions_only_selects_exact_entry_and_narrow_first_grids', () => {
		expect(captureArguments('/declared/capture', 'static-sessions')).toEqual([
			'/declared/capture',
			'compiled_compact_current_sessions_storyboard_exports_real_tui',
			'--exact',
			'--ignored',
			'--nocapture',
			'--test-threads=1',
		]);
		expect(staticSurfaces('static-sessions')).toEqual(['sessions']);
		expect(blankScene('static-sessions')).toEqual(['compact-sessions-all', 'Sessions']);
		expect(staticViewports('static-sessions')).toEqual([
			{ columns: 32, rows: 24 },
			{ columns: 40, rows: 24 },
			{ columns: 48, rows: 12 },
			{ columns: 48, rows: 24 },
			{ columns: 76, rows: 30 },
			{ columns: 120, rows: 36 },
		]);
	});
	test('static_budget_accounts_for_every_grid_image_and_cleanup', () => {
		expect(staticCaptureBudgetSeconds('static-quota')).toBe(90 + 6 * (90 + 40) + (85 + 36) * 30);
		expect(staticCaptureBudgetSeconds('static-both')).toBe(90 + 12 * (90 + 40) + (218 + 74) * 30);
		expect(staticCaptureBudgetSeconds('static-sessions')).toBe(
			90 + 6 * (90 + 40) + (133 + 38) * 30,
		);
		for (const selection of ['cua-quota', 'cua-sessions']) {
			expect(() =>
				staticCaptureBudgetSeconds(selection === 'cua-quota' ? 'cua-quota' : 'cua-sessions'),
			).toThrow('no static image');
		}
	});
	test.each([
		['new-machine', 'new'],
		['fork-choice', 'fork'],
		['codex-details', 'details'],
		['model-provider-choice', 'filters'],
		['provider-no-match', 'filters'],
		['disabled-choice', 'filters'],
		['hidden-choice', 'filters'],
		['machine-sources', 'machines'],
	])('gallery_retains_new_fork_and_choice_dialog_identity %s', (checkpoint, expected) => {
		if (checkpoint === undefined) throw new Error('missing literal');
		expect(checkpointDialog(checkpoint)).toBe(expected);
	});
	test('resize_readback_allows_new_dimensions_but_rejects_replaced_pty', () => {
		const before = cuaReadbackSchema.parse({
			surface: 'sessions',
			identity: {
				pid: 123,
				uid: 501,
				group: 123,
				birth: 'first start',
				executable: '/fixture/harness',
			},
			viewport: { columns: 120, rows: 36 },
			source_sha256: 'a'.repeat(64),
			node_sha256: '0'.repeat(64),
			binary_sha256: 'b'.repeat(64),
			observed_at: '2026-10-18T00:00:00Z',
		});
		const after = { ...before, viewport: { columns: 48, rows: 12 } };
		requireSamePty(after, before);
		for (const identity of [
			{ ...before.identity, pid: 124 },
			{ ...before.identity, birth: 'reused PID new start' },
		])
			expect(() => requireSamePty({ ...after, identity }, before)).toThrow(
				'replaced the declared PTY',
			);
	});
	test('live_cua_ready_url_rejects_foreign_or_credential_destinations', () => {
		const fields = {
			kind: 'listener-ready-not-ui',
			surface: 'sessions',
			ttyd_pid: 123,
			node_sha256: '0'.repeat(64),
			binary_sha256: 'a'.repeat(64),
			ttyd_sha256: 'b'.repeat(64),
			child_pid_file: '/fixture/quota-child.pid',
			viewport_file: '/fixture/quota-child.viewport',
			stop_file: '/fixture/cua-stop',
			lifetime_seconds: 600,
			artifact_export: {
				base_url: `http://127.0.0.1:2346/export/${'a'.repeat(64)}/`,
				directory: '/fixture/original-images',
				scenes: ['sessions-details-120x36'],
				maximum_bytes: 8388608,
				mime_type: 'image/jpeg',
				source_sha256: 'c'.repeat(64),
			},
		};
		cuaReadySchema.parse({ ...fields, url: 'http://127.0.0.1:2345/' });
		for (const url of [
			'https://example.com/',
			'http://user:secret@127.0.0.1:2345/',
			'http://127.0.0.1:2345/other',
		])
			expect(() => cuaReadySchema.parse({ ...fields, url })).toThrow();
		for (const change of [
			{ base_url: `https://example.com/export/${'a'.repeat(64)}/` },
			{ scenes: ['../outside'] },
			{ scenes: ['same', 'same'] },
			{ mime_type: 'image/png' },
		])
			expect(() =>
				cuaReadySchema.parse({
					...fields,
					url: 'http://127.0.0.1:2345/',
					artifact_export: { ...fields.artifact_export, ...change },
				}),
			).toThrow();
	});
	test('mutation_provider_call_or_unsettled_cleanup_receipt_cannot_pass', () => {
		const receipt = receiptLiteral();
		filterReceiptSchema.parse(receipt);
		for (const change of [
			{ provider_requests: 1 },
			{ child_exit: 1 },
			{ owned_processes_stopped: false },
			{ state_and_secrets_unchanged: false },
		])
			expect(() => filterReceiptSchema.parse({ ...receipt, ...change })).toThrow();
		const parsed = filterReceiptSchema.parse(receipt);
		const first = parsed.images[0];
		if (first === undefined) throw new Error('fixture incomplete');
		expect(() =>
			filterReceiptSchema.parse({ ...receipt, images: [...parsed.images, first] }),
		).toThrow('duplicate checkpoint');
	});
	test('negative_control_that_accepted_wrong_image_is_rejected', () => {
		const fields = {
			checkpoint: 'compact-sessions-claude',
			recognizedLines: ['wrong screen'],
			recognizedFieldPairs: [],
			choiceFocusMeasurements: [],
			missingMarkers: [],
			unexpectedMarkers: [],
		};
		expect(() => imageRejectionSchema.parse(fields)).toThrow('accepted the wrong scene');
		imageRejectionSchema.parse({ ...fields, unexpectedMarkers: ['Codex Alpha'] });
	});
	test('original_choice_focus_requires_label_and_same_row_glyph', () => {
		requireSelectedChoice(
			imageObservationSchema.parse(focusObservation([originalFocus])),
			'hidden',
		);
		for (const change of [
			{ labelYellowPixels: 0 },
			{ glyphYellowPixels: 0 },
			{ glyphColumns: 0 },
			{ glyphRows: 0 },
			{ label: 'shown' },
		])
			expect(() =>
				requireSelectedChoice(
					imageObservationSchema.parse(focusObservation([{ ...originalFocus, ...change }])),
					'hidden',
				),
			).toThrow('glyph/color');
		for (const measurements of [
			[],
			[originalFocus, originalFocus],
			[originalFocus, { ...originalFocus, label: 'shown' }],
		])
			expect(() =>
				requireSelectedChoice(
					imageObservationSchema.parse(focusObservation(measurements)),
					'hidden',
				),
			).toThrow('glyph/color');
		for (const change of [{ label: 'OpenAI' }, { glyphYellowPixels: -1 }, { glyphRows: true }])
			expect(() => choiceFocusSchema.parse({ ...originalFocus, ...change })).toThrow();
		const fields = imageObservationSchema.parse(focusObservation([originalFocus]));
		const { choiceFocusMeasurements, ...missing } = fields;
		expect(choiceFocusMeasurements.length).toBe(1);
		expect(() => imageObservationSchema.parse(missing)).toThrow();
	});
	test('scene_oracle_cannot_hide_missing_or_unexpected_markers', () => {
		for (const change of [{ missingMarkers: ['Apply'] }, { unexpectedMarkers: ['Quota Help'] }])
			expect(() =>
				imageObservationSchema.parse({
					...imageObservationSchema.parse(focusObservation([])),
					...change,
				}),
			).toThrow();
	});
	test('compact_route_never_claims_all_menu_or_resize_coverage', () => {
		const gaps = coverageGaps();
		for (const name of ['sessions/new', 'quota/reset-result', 'resize continuity'])
			expect(gaps).toContain(name);
		expect(gaps.some((gap) => gap.includes('48x12'))).toBe(true);
		expect(gaps.some((gap) => gap.includes('40x24') && gap.includes('32x24'))).toBe(true);
		expect(gaps.some((gap) => gap.includes('actual iPhone'))).toBe(true);
	});
	test('strict schemas reject boolean integers, unknown fields and malformed artifacts', () => {
		expect(() => choiceFocusSchema.parse({ ...originalFocus, glyphRows: true })).toThrow();
		expect(() => artifactBindingSchema.parse({ path: '/fixture', sha256: 'wrong' })).toThrow();
		expect(() => capturePlanSchema.parse({ unexpected: 'field' })).toThrow();
	});
});
test('parsed boundary models reject top-level evidence mutation', () => {
	const binding = artifactBindingSchema.parse({ path: '/fixture', sha256: 'a'.repeat(64) });
	expect(Object.isFrozen(binding)).toBe(true);
	expect(Reflect.set(binding, 'sha256', 'b'.repeat(64))).toBe(false);
	expect(binding.sha256).toBe('a'.repeat(64));
	const receipt = filterReceiptSchema.parse(receiptLiteral());
	expect(Object.isFrozen(receipt)).toBe(true);
	expect(Reflect.set(receipt, 'provider_requests', 1)).toBe(false);
});
