import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, expect, test } from 'vitest';
import { z } from 'zod';

import { collectImages } from './capture-collection.ts';
import { validateImageQualification } from './capture-execution.ts';
import { captureArguments, requireOneTest, staticSurfaces } from './capture-selection.ts';
import type { CaptureSelection } from './capture-selection.ts';
import { filterReceiptSchema, imageObservationSchema } from './evidence-schema.ts';
import { collectCuaArtifacts } from './original-artifacts.ts';
import {
	negativeRecord,
	prepareStaticReceipts,
	protocolPlan,
	saveJson,
	temporaryRoot,
} from './receipt-fixtures.ts';
import { captureArtifact, readJson } from './source-binding.ts';
const roots: string[] = [];
function rootDirectory(): string {
	const root = temporaryRoot();
	roots.push(root);
	return root;
}
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});
function updateObject(path: string, change: (value: Record<string, unknown>) => void): void {
	const value = z.record(z.string(), z.unknown()).parse(readJson(path));
	change(value);
	saveJson(path, value);
}
const selectionCases: readonly {
	readonly selection: CaptureSelection;
	readonly count: number;
	readonly surfaces: readonly string[];
}[] = [
	{ selection: 'static-both', count: 218, surfaces: ['quota', 'sessions'] },
	{ selection: 'static-quota', count: 85, surfaces: ['quota'] },
	{ selection: 'static-sessions', count: 133, surfaces: ['sessions'] },
];
test('sessions_accepts_measured_ordered_details_pose_sets', () => {
	const plan = prepareStaticReceipts(rootDirectory(), 'static-sessions');
	const images = collectImages(plan, 'static-sessions');
	expect(images).toHaveLength(133);
	expect(
		images.filter((image) => image.journey.startsWith('compact-sessions-details')),
	).toHaveLength(19);
});
test.each([
	'missing',
	'extra',
	'reordered',
	'foreign-build',
	'wrong-id',
	'missing-title',
	'cross-pose',
])('sessions_details_rejects_incomplete_foreign_or_cross_pose_evidence %s', (failure) => {
	const plan = prepareStaticReceipts(rootDirectory(), 'static-sessions');
	const folder = join(plan.output, 'sessions-compact-48x12');
	const path = join(folder, 'filter-receipt.json');
	const receipt = filterReceiptSchema.parse(readJson(path));
	if (['missing', 'extra', 'reordered'].includes(failure)) {
		const entries = [...receipt.images];
		const index = entries.findIndex(
			(entry) => entry.checkpoint === 'compact-sessions-details-identity',
		);
		const entry = entries[index];
		const next = entries[index + 1];
		if (entry === undefined || next === undefined) throw new Error('fixture incomplete');
		if (failure === 'missing') entries.splice(index, 1);
		else if (failure === 'extra') entries.splice(index, 0, entry);
		else {
			entries[index] = next;
			entries[index + 1] = entry;
		}
		saveJson(path, { ...receipt, images: entries });
	} else if (failure === 'foreign-build')
		saveJson(path, { ...receipt, binary_sha256: 'f'.repeat(64) });
	else {
		const state = failure === 'missing-title' ? 'details-title' : 'details-identity';
		const observationPath = join(folder, `compact-sessions-${state}.image-observation.json`);
		const observation = imageObservationSchema.parse(readJson(observationPath));
		saveJson(
			observationPath,
			failure === 'cross-pose'
				? { ...observation, checkpoint: 'compact-sessions-details-title' }
				: {
						...observation,
						recognizedFieldPairs:
							failure === 'wrong-id'
								? [
										'details context claude gamma [000.0011]',
										'details session id 00000000-0000-4000-8000-000000000012',
									]
								: ['details context claude gamma [000.0011]'],
					},
		);
	}
	expect(() => collectImages(plan, 'static-sessions')).toThrow();
});
test.each([
	{ pairs: [] },
	{ pairs: ['client cursor'] },
	{ pairs: ['draft codex'] },
	{ pairs: ['client codex', 'client cursor'] },
])('sessions_chooser_rejects_missing_or_wrong_client_association $pairs', ({ pairs }) => {
	const plan = prepareStaticReceipts(rootDirectory(), 'static-both');
	const path = join(
		plan.output,
		'sessions-compact-120x36/compact-sessions-chooser.image-observation.json',
	);
	updateObject(path, (value) => {
		value['recognizedFieldPairs'] = pairs;
	});
	expect(() => collectImages(plan)).toThrow('associated');
});
test.each(selectionCases)(
	'static_selection_collects_every_declared_scene_and_grid $selection',
	({ selection, count, surfaces }) => {
		const plan = prepareStaticReceipts(rootDirectory(), selection);
		const images = collectImages(plan, selection);
		expect(images).toHaveLength(count);
		expect([...new Set(images.map((image) => image.screen))].toSorted()).toEqual(surfaces);
		expect(new Set(images.map((image) => `${image.columns}x${image.rows}`))).toEqual(
			new Set(['120x36', '76x30', '48x24', '48x12', '40x24', '32x24']),
		);
	},
);
test.each(['missing-grid', 'extra-quota', 'wrong-surface', 'missing-negative'])(
	'sessions_only_rejects_missing_extra_wrong_and_unchecked_receipts %s',
	(failure) => {
		const plan = prepareStaticReceipts(rootDirectory(), 'static-sessions');
		const folder = join(plan.output, 'sessions-compact-32x24');
		const path = join(folder, 'filter-receipt.json');
		if (failure === 'missing-grid') rmSync(path);
		else if (failure === 'extra-quota') {
			const extra = join(plan.output, 'quota-compact-32x24');
			mkdirSync(extra);
			writeFileSync(join(extra, 'filter-receipt.json'), readFileSync(path));
		} else if (failure === 'wrong-surface')
			updateObject(path, (value) => {
				value['surface'] = 'quota';
			});
		else
			rmSync(join(folder, 'reject-compact-sessions-claude-blank-startup.image-observation.json'));
		expect(() => collectImages(plan, 'static-sessions')).toThrow();
	},
);
const quotaFailures = [
	'missing-grid',
	'missing-image',
	'extra-image',
	'extra-receipt',
	'missing-scene',
	'missing-choice-scene',
	'reordered-choice-scenes',
	'missing-choice-negative',
	'missing-pool-negative',
	'missing-pool-complete-scene',
	'missing-choice-focus',
	'wrong-choice-focus',
	'wrong-field-association',
	'missing-field-association',
	'wrong-scene',
	'extra-scene',
	'wrong-target',
	'changed-checkpoint-source',
	'missing-checkpoint-source',
	'provider-effect',
	'accepted-negative',
];
test.each(quotaFailures)(
	'quota_collection_rejects_missing_wrong_extra_scene_target_and_negative %s',
	(failure) => {
		const plan = prepareStaticReceipts(rootDirectory(), 'static-quota');
		const folder = join(plan.output, 'quota-compact-48x12');
		const path = join(folder, 'filter-receipt.json');
		const receipt = filterReceiptSchema.parse(readJson(path));
		if (failure === 'missing-grid') rmSync(path);
		else if (failure === 'changed-checkpoint-source')
			writeFileSync(
				join(
					plan.repository,
					'crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/verify_screen_checkpoint.swift',
				),
				'different scene expectations',
			);
		else if (failure === 'missing-checkpoint-source')
			updateObject(path, (value) => {
				delete value['verifier_checkpoint_source_sha256'];
			});
		else if (failure === 'missing-image') rmSync(join(folder, 'compact-quota-details.png'));
		else if (failure === 'extra-image') writeFileSync(join(folder, 'unexpected.png'), 'not UI');
		else if (failure === 'extra-receipt') {
			const extra = join(plan.output, 'sessions-compact-120x36');
			mkdirSync(extra);
			writeFileSync(join(extra, 'filter-receipt.json'), readFileSync(path));
		} else if (failure === 'missing-pool-complete-scene')
			rmSync(join(plan.output, 'quota-compact-120x36/compact-quota-pool-details-complete.png'));
		else if (failure === 'missing-pool-negative')
			rmSync(
				join(folder, 'reject-compact-quota-pool-details-compact-quota-all.image-observation.json'),
			);
		else if (failure === 'missing-choice-negative')
			rmSync(
				join(
					folder,
					'reject-compact-quota-hidden-choice-compact-quota-disabled-choice.image-observation.json',
				),
			);
		else if (failure === 'missing-choice-focus' || failure === 'wrong-choice-focus')
			saveJson(join(folder, 'compact-quota-hidden-choice.image-observation.json'), {
				checkpoint: 'compact-quota-hidden-choice',
				recognizedLines: ['Account filters', 'Shown', 'Hidden', 'Cancel Apply'],
				recognizedFieldPairs: [],
				choiceFocusMeasurements:
					failure === 'missing-choice-focus'
						? []
						: [
								{
									label: 'shown',
									labelYellowPixels: 131,
									glyphYellowPixels: 25,
									glyphColumns: 5,
									glyphRows: 12,
									labelHeight: 20,
								},
							],
				missingMarkers: [],
				unexpectedMarkers: [],
			});
		else if (failure === 'wrong-field-association' || failure === 'missing-field-association')
			updateObject(join(folder, 'compact-quota-chooser.image-observation.json'), (value) => {
				if (failure === 'missing-field-association') delete value['recognizedFieldPairs'];
				else value['recognizedFieldPairs'] = ['account provider claude', 'disabled accounts shown'];
			});
		else if (failure === 'accepted-negative')
			saveJson(join(folder, 'reject-compact-quota-claude-blank-startup.image-observation.json'), {
				checkpoint: 'compact-quota-claude',
				recognizedLines: [],
				recognizedFieldPairs: [],
				choiceFocusMeasurements: [],
				missingMarkers: [],
				unexpectedMarkers: [],
			});
		else {
			let images = [...receipt.images];
			if (failure === 'missing-scene') images.pop();
			else if (failure === 'missing-choice-scene')
				images = images.filter((image) => image.checkpoint !== 'compact-quota-hidden-choice');
			else if (failure === 'reordered-choice-scenes') {
				const first = images[4];
				const second = images[5];
				if (first === undefined || second === undefined) throw new Error('fixture incomplete');
				images[4] = second;
				images[5] = first;
			} else if (failure === 'wrong-scene') {
				const last = images.pop();
				if (last === undefined) throw new Error('fixture incomplete');
				images.push({ ...last, checkpoint: 'compact-quota-unexpected' });
			} else if (failure === 'extra-scene')
				images.push({ checkpoint: 'compact-quota-extra', png_sha256: 'b'.repeat(64) });
			saveJson(path, {
				...receipt,
				images,
				...(failure === 'wrong-target' ? { binary_sha256: 'b'.repeat(64) } : {}),
				...(failure === 'provider-effect' ? { provider_requests: 1 } : {}),
			});
		}
		expect(() => collectImages(plan, 'static-quota')).toThrow();
	},
);
test('static_surface_and_cua_modes_are_mutually_exclusive', () => {
	for (const flags of [
		['--quota-only', '--cua-surface', 'quota'],
		['--sessions-only', '--cua-surface', 'sessions'],
		['--sessions-only', '--quota-only'],
	]) {
		const result = spawnSync(
			process.execPath,
			[fileURLToPath(new URL('./capture-run.ts', import.meta.url)), 'unused-plan.json', ...flags],
			{ encoding: 'utf8' },
		);
		expect(result.status).toBe(2);
		expect(result.stderr).toContain('not allowed with argument');
	}
	expect(() => staticSurfaces('cua-quota')).toThrow('no static image');
	expect(() => staticSurfaces('cua-sessions')).toThrow('no static image');
});
test('quota_blank_oracle_cannot_use_sessions_or_accept_blank', () => {
	const result = JSON.stringify({
		...z.record(z.string(), z.unknown()).parse(negativeRecord('compact-quota-all')),
		missingMarkers: ['Quota status'],
	});
	validateImageQualification(1, result, 'static-quota');
	for (const wrong of [
		result.replace('compact-quota-all', 'compact-sessions-all'),
		result.replace('Quota status', 'Sessions'),
	])
		expect(() => validateImageQualification(1, wrong, 'static-quota')).toThrow(
			'exact declared scene',
		);
	for (const exit of [0, 2, 133])
		expect(() => validateImageQualification(exit, result, 'static-quota')).toThrow(
			'unavailable or blank accepted',
		);
});
test('quota_only_selects_its_exact_declared_entry_without_sessions', () => {
	expect(captureArguments('/declared/capture', 'static-quota')).toEqual([
		'/declared/capture',
		'compiled_compact_current_quota_storyboard_exports_real_tui',
		'--exact',
		'--ignored',
		'--nocapture',
		'--test-threads=1',
	]);
});
test('original_cua_collector_rejects_modified_bytes_and_wrong_count', () => {
	const plan = { ...protocolPlan(rootDirectory()), cua_artifact_scenes: ['quota-details-120x36'] };
	const folder = join(plan.output, 'cua-quota/original-images');
	mkdirSync(folder, { recursive: true });
	const scene = 'quota-details-120x36';
	const image = join(folder, `${scene}.jpg`);
	writeFileSync(image, Buffer.from([255, 216, 42, 255, 217]));
	const record = {
		kind: 'original-cua-jpeg-bytes-only',
		scene,
		filename: `${scene}.jpg`,
		bytes: 5,
		sha256: captureArtifact(image).sha256,
		encoding_round_trip: true,
	};
	saveJson(join(folder, `${scene}.receipt.json`), record);
	expect(collectCuaArtifacts(plan, 'quota', 1)[0]?.result).toBe(
		'original-bytes-retained-ui-judgment-separate',
	);
	const scenes = ['wide-draft', 'narrow-draft', 'restored-draft'];
	plan.cua_artifact_scenes.push(...scenes);
	expect(() => collectCuaArtifacts(plan, 'quota', 1)).toThrow('count or declared');
	for (const extra of scenes) {
		writeFileSync(join(folder, `${extra}.jpg`), readFileSync(image));
		saveJson(join(folder, `${extra}.receipt.json`), {
			...record,
			scene: extra,
			filename: `${extra}.jpg`,
		});
	}
	expect(collectCuaArtifacts(plan, 'quota', 4)).toHaveLength(4);
	expect(() => collectCuaArtifacts(plan, 'quota', 2)).toThrow('count or declared');
	for (const name of ['undeclared.jpg', 'undeclared.receipt.json']) {
		writeFileSync(
			join(folder, name),
			name.endsWith('.jpg') ? readFileSync(image) : JSON.stringify(record),
		);
		expect(() => collectCuaArtifacts(plan, 'quota', 4)).toThrow('count or declared');
		rmSync(join(folder, name));
	}
	const receipt = join(folder, 'restored-draft.receipt.json');
	const original = readFileSync(receipt);
	rmSync(receipt);
	expect(() => collectCuaArtifacts(plan, 'quota', 3)).toThrow('count or declared');
	writeFileSync(receipt, original);
	const missing = join(folder, 'restored-draft.jpg');
	const pixels = readFileSync(missing);
	rmSync(missing);
	expect(() => collectCuaArtifacts(plan, 'quota', 4)).toThrow();
	writeFileSync(missing, pixels);
	writeFileSync(image, Buffer.from([255, 216, 43, 255, 217]));
	expect(() => collectCuaArtifacts(plan, 'quota', 4)).toThrow('digest mismatch');
});
test('image_qualification_requires_real_negative_not_harness_failure', () => {
	const negative = JSON.stringify({
		...z.record(z.string(), z.unknown()).parse(negativeRecord('compact-sessions-all')),
		missingMarkers: ['Sessions'],
	});
	validateImageQualification(1, negative);
	for (const exit of [0, 2, 133])
		expect(() => validateImageQualification(exit, negative)).toThrow(
			'unavailable or blank accepted',
		);
	expect(() =>
		validateImageQualification(1, negative.replace('compact-sessions-all', 'compact-quota-all')),
	).toThrow('exact declared scene');
});
test('capture_selection_is_exact_ignored_and_serial', () => {
	const args = captureArguments('/declared/capture');
	expect(args).toEqual([
		'/declared/capture',
		'compiled_compact_current_storyboard_exports_both_real_tuis',
		'--exact',
		'--ignored',
		'--nocapture',
		'--test-threads=1',
	]);
	expect(args).not.toContain('cargo');
});
test('empty_or_broadened_discovery_is_not_a_pass', () => {
	requireOneTest(
		'compiled_compact_current_storyboard_exports_both_real_tuis: test\n1 test, 0 benchmarks',
	);
	for (const discovery of [
		'0 tests, 0 benchmarks',
		'unrelated: test\n1 test, 0 benchmarks',
		'compiled_compact_current_storyboard_exports_both_real_tuis: test\nextra: test',
	])
		expect(() => requireOneTest(discovery)).toThrow('exact capture test');
});
test.each(['missing', 'changed'])('compact receipt rejects unbound Node runtime %s', (failure) => {
	const plan = prepareStaticReceipts(rootDirectory(), 'static-quota');
	const path = join(plan.output, 'quota-compact-120x36/filter-receipt.json');
	updateObject(path, (value) => {
		if (failure === 'missing') delete value['node_sha256'];
		else value['node_sha256'] = '0'.repeat(64);
	});
	expect(() => collectImages(plan, 'static-quota')).toThrow();
});
