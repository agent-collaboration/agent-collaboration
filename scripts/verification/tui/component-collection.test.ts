import { readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';
import { z } from 'zod';

import { runComponentCapture } from './capture-execution.ts';
import { collectComponentImages } from './component-collection.ts';
import { controlledExecutable, initializedSourceRepository } from './controlled-child-fixtures.ts';
import {
	componentPlanSchema,
	componentReceiptSchema,
	imageObservationSchema,
} from './evidence-schema.ts';
import { prepareComponentReceipts, temporaryRoot, saveJson } from './receipt-fixtures.ts';
import { componentWorkSeconds, componentStates, componentNegatives } from './scene-contracts.ts';
import { captureArtifact, readJson, sourceBinding } from './source-binding.ts';
const roots: string[] = [];
function rootDirectory(): string {
	const root = temporaryRoot();
	roots.push(root);
	return root;
}
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});
test.each(['none', 'source', 'artifact'])(
	'collection_rechecks_real_source_and_artifact_after_driver %s',
	async (mutation) => {
		const root = rootDirectory();
		const repository = initializedSourceRepository();
		roots.push(repository);
		const prepared = prepareComponentReceipts(repository);
		const saved = join(root, 'saved-receipts');
		renameSync(prepared.output, saved);
		const source = join(repository, 'crates/declared-source.rs');
		writeFileSync(source, 'before driver\n');
		const target = join(repository, 'protocol-only-target');
		const checker = controlledExecutable(
			root,
			'checker',
			`process.stdout.write(JSON.stringify({checkpoint:'component-quota-policy-saved',recognizedLines:[],recognizedFieldPairs:[],choiceFocusMeasurements:[],missingMarkers:['marker'],unexpectedMarkers:[]}));process.exitCode=1;`,
		);
		const fixture = controlledExecutable(
			root,
			'component-fixture',
			`const name='quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture';process.stdout.write(name+': test\\n');`,
		);
		// This real controlled driver implements only libtest discovery/publication.
		// Real target hashes, Git enumeration, collection and post-run validation stay real.
		const driver = controlledExecutable(
			root,
			'capture-driver',
			`const fs=require('node:fs');if(process.argv.at(-1)==='--list'){process.stdout.write(process.argv[2]+': test\\n');}else{fs.cpSync(${JSON.stringify(saved)},process.env.QUOTA_STORYBOARD_OUTPUT,{recursive:true});${mutation === 'source' ? `fs.writeFileSync(${JSON.stringify(source)},'changed during capture');` : mutation === 'artifact' ? `fs.writeFileSync(${JSON.stringify(fixture)},'changed during capture');` : ''}}`,
		);
		const plan = componentPlanSchema.parse({
			...prepared,
			source: sourceBinding(repository),
			capture_driver: captureArtifact(driver),
			component_binary: captureArtifact(fixture),
			image_verifier: captureArtifact(checker),
		});
		const receiptPath = join(saved, 'quota-component-normal-120x36/component-receipt.json');
		const receipt = componentReceiptSchema.parse(readJson(receiptPath));
		saveJson(receiptPath, {
			...receipt,
			binary_sha256: plan.component_binary.sha256,
			verifier_sha256: plan.image_verifier.sha256,
		});
		expect(readFileSync(target, 'utf8')).toBe('protocol only');
		if (mutation === 'none') {
			await runComponentCapture(plan);
			expect(readJson(join(plan.output, 'component-gallery-manifest.json'))).toMatchObject({
				complete: false,
			});
		} else {
			await expect(runComponentCapture(plan)).rejects.toThrow(/source changed|artifact changed/u);
			expect(() => readFileSync(join(plan.output, 'component-gallery-manifest.json'))).toThrow();
		}
	},
);
test('complete_protocol_and_finite_budget', () => {
	const plan = prepareComponentReceipts(rootDirectory());
	const images = collectComponentImages(plan);
	expect(images).toHaveLength(43);
	for (const [scene, dialog] of [
		['confirm-no', 'Confirm reset'],
		['confirm-no-returned', 'Options · Resets'],
		['reset-result-bottom', 'Reset result'],
		['reset-result-consumption', 'Reset result'],
		['policy-saved', 'Credit usage'],
	])
		expect(images.find((image) => image.journey === `component-quota-${scene}`)?.dialog).toBe(
			dialog,
		);
	const limits = images.find(
		(image) => image.journey === 'component-quota-confirm-help-identity',
	)?.limits;
	expect(limits).toContain('Personal original-image inspection');
	expect(limits).toContain('literal tag brackets');
	expect(limits).toContain('raw OCR is unchanged');
	expect(componentWorkSeconds(plan)).toBe(1960);
	expect(
		componentWorkSeconds({
			...plan,
			grids: [
				{ columns: 120, rows: 36 },
				{ columns: 76, rows: 30 },
				{ columns: 48, rows: 24 },
				{ columns: 48, rows: 12 },
				{ columns: 40, rows: 24 },
				{ columns: 32, rows: 24 },
			],
		}) + 90,
	).toBe(11850);
	for (const grids of [
		[],
		[{ columns: 48, rows: 13 }],
		[{ columns: 40, rows: 12 }],
		[{ columns: 32, rows: 12 }],
		[
			{ columns: 48, rows: 12 },
			{ columns: 48, rows: 12 },
		],
		Array.from({ length: 7 }, () => ({ columns: 32, rows: 24 })),
	])
		expect(() => componentPlanSchema.parse({ ...plan, grids })).toThrow();
});
test('hard_case_scenario_collects_only_its_own_scenes_and_controls', () => {
	const plan = prepareComponentReceipts(
		rootDirectory(),
		{ columns: 120, rows: 36 },
		'visual-hard-cases',
	);
	const images = collectComponentImages(plan);
	expect(images.map((image) => image.journey)).toEqual(
		'hard-browse hard-beta-windows hard-beta-extra hard-pool-details hard-pool-details-complete hard-pool-details-middle hard-pool-details-bottom hard-pool-closed hard-details'
			.split(' ')
			.map((state) => `component-quota-${state}`),
	);
	expect(
		images.find((image) => image.journey === 'component-quota-hard-pool-details')?.dialog,
	).toBe('Pool details');
	expect(images.find((image) => image.journey === 'component-quota-hard-details')?.dialog).toBe(
		'Account details',
	);
	expect(
		images.find((image) => image.journey === 'component-quota-hard-beta-windows')?.dialog,
	).toBe('browse');
	expect(images.find((image) => image.journey === 'component-quota-hard-beta-extra')?.dialog).toBe(
		'browse',
	);
	expect(componentWorkSeconds(plan)).toBe(240 + 30 * (9 + 6) + 40);
	const partial = { ...plan, grids: [{ columns: 76, rows: 30 }] };
	expect(componentStates(partial, { columns: 76, rows: 30 })).toHaveLength(8);
	expect(componentWorkSeconds(partial)).toBe(240 + 30 * (8 + 6) + 40);
	const short = { ...plan, grids: [{ columns: 48, rows: 12 }] };
	expect(componentStates(short, { columns: 48, rows: 12 })).toHaveLength(14);
	expect(componentNegatives(short, { columns: 48, rows: 12 })).toHaveLength(9);
	expect(componentWorkSeconds(short)).toBe(240 + 30 * (14 + 9) + 40);
});
test.each(['normal-kind', 'missing-negative', 'missing-scene'])(
	'hard case rejects foreign scenes %s',
	(failure) => {
		const plan = prepareComponentReceipts(
			rootDirectory(),
			{ columns: 120, rows: 36 },
			'visual-hard-cases',
		);
		const folder = join(plan.output, 'quota-component-hard-cases-120x36');
		const path = join(folder, 'component-receipt.json');
		const receipt = componentReceiptSchema.parse(readJson(path));
		if (failure === 'normal-kind')
			saveJson(path, { ...receipt, kind: 'compiled-quota-component-normal' });
		else if (failure === 'missing-scene')
			saveJson(path, { ...receipt, images: receipt.images.slice(1) });
		else
			rmSync(
				join(
					folder,
					'reject-component-quota-hard-pool-details-component-quota-hard-beta-windows.image-observation.json',
				),
			);
		expect(() => collectComponentImages(plan)).toThrow();
	},
);
test.each([40, 32])(
	'phone_receipt_cannot_substitute_a_clamped_48_column_viewport %s',
	(columns) => {
		const plan = prepareComponentReceipts(rootDirectory(), { columns, rows: 24 });
		const images = collectComponentImages(plan);
		expect(images).toHaveLength(43);
		expect(
			images.every((image) => image.viewport.columns === columns && image.viewport.rows === 24),
		).toBe(true);
		const path = join(plan.output, `quota-component-normal-${columns}x24/component-receipt.json`);
		saveJson(path, { ...componentReceiptSchema.parse(readJson(path)), columns: 48 });
		expect(() => collectComponentImages(plan)).toThrow('viewport mismatch');
	},
);
const failures = [
	'draft-is-not-saved',
	'floor-draft-is-not-saved',
	'missing-floor-draft-negative',
	'missing-stale-refresh-negative',
	'missing-negative',
	'missing-returned-negative',
	'missing-help-bottom-negative',
	'missing-details-bottom-negative',
	'missing-result-bottom-negative',
	'missing-consumption-positive',
	'missing-consumption-negative',
	'missing-compact-credit-negative',
	'missing-short-window-negative',
	'missing-lower-pace-negative',
	'wrong-binary',
	'changed-source',
	'reordered',
	'missing-source',
	'provider-effect',
	'extra-image',
	'first-frame-diagnostic',
];
test.each(failures)(
	'wrong_saved_values_effects_sources_or_scene_cannot_earn_proof %s',
	(failure) => {
		const plan = prepareComponentReceipts(rootDirectory());
		const folder = join(plan.output, 'quota-component-normal-120x36');
		const path = join(folder, 'component-receipt.json');
		const receipt = componentReceiptSchema.parse(readJson(path));
		if (failure === 'draft-is-not-saved' || failure === 'floor-draft-is-not-saved') {
			const scene = failure === 'draft-is-not-saved' ? 'policy-saved' : 'floor-saved';
			const observationPath = join(folder, `component-quota-${scene}.image-observation.json`);
			saveJson(observationPath, {
				...imageObservationSchema.parse(readJson(observationPath)),
				recognizedFieldPairs: scene === 'policy-saved' ? ['use credits disallow'] : [],
			});
		} else if (failure === 'changed-source')
			writeFileSync(
				join(
					plan.repository,
					'crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/verify_screen_checkpoint.swift',
				),
				'changed markers',
			);
		else if (failure === 'extra-image') writeFileSync(join(folder, 'extra.png'), 'stale');
		else if (failure === 'missing-consumption-positive')
			rmSync(join(folder, 'component-quota-reset-result-consumption.png'));
		else if (failure.startsWith('missing-') && failure !== 'missing-source') {
			const paths: Record<string, string> = {
				'missing-floor-draft-negative': 'floor-saved-component-quota-floor-draft',
				'missing-stale-refresh-negative':
					'credits-refreshed-balance-component-quota-credits-balance',
				'missing-negative': 'policy-saved-component-quota-policy-cancelled',
				'missing-returned-negative': 'returned-component-quota-reset-result',
				'missing-help-bottom-negative': 'help-bottom-component-quota-help-top',
				'missing-details-bottom-negative': 'details-bottom-component-quota-details-top',
				'missing-result-bottom-negative': 'reset-result-bottom-component-quota-confirm-yes',
				'missing-consumption-negative': 'reset-result-consumption-component-quota-confirm-yes',
				'missing-compact-credit-negative': 'browse-credit-window-component-quota-next-account',
				'missing-short-window-negative': 'browse-pace-heading-component-quota-next-account',
				'missing-lower-pace-negative': 'browse-pace-value-component-quota-reset-result',
			};
			const suffix = paths[failure];
			if (suffix === undefined) throw new Error('unknown negative');
			rmSync(join(folder, `reject-component-quota-${suffix}.image-observation.json`));
		} else {
			const value = z.record(z.string(), z.unknown()).parse(receipt);
			if (failure === 'wrong-binary') value['binary_sha256'] = '0'.repeat(64);
			else if (failure === 'missing-source') delete value['checkpoint_source_sha256'];
			else if (failure === 'provider-effect') value['external_requests'] = 1;
			else if (failure === 'first-frame-diagnostic')
				value['kind'] = 'compiled-quota-first-frame-diagnostic';
			else if (failure === 'reordered') {
				const images = [...receipt.images];
				const first = images[18];
				const second = images[20];
				if (first === undefined || second === undefined) throw new Error('incomplete fixture');
				images[18] = second;
				images[20] = first;
				value['images'] = images;
			}
			saveJson(path, value);
		}
		expect(() => collectComponentImages(plan)).toThrow();
	},
);
test.each(['missing', 'changed'])(
	'component receipt rejects unbound Node runtime %s',
	(failure) => {
		const plan = prepareComponentReceipts(rootDirectory());
		const path = join(plan.output, 'quota-component-normal-120x36/component-receipt.json');
		const receipt = componentReceiptSchema.parse(readJson(path));
		const altered: Record<string, unknown> = { ...receipt };
		if (failure === 'missing') delete altered['node_sha256'];
		else altered['node_sha256'] = '0'.repeat(64);
		saveJson(path, altered);
		expect(() => collectComponentImages(plan)).toThrow();
	},
);
