import { readdirSync } from 'node:fs';
import { join } from 'node:path';

import {
	componentReceiptSchema,
	imageObservationSchema,
	imageRejectionSchema,
} from './evidence-schema.ts';
import type { ComponentPlan, TerminalViewport } from './evidence-schema.ts';
import {
	findReceiptPaths,
	requireExactOrder,
	requireExactSet,
	requireOriginalPng,
} from './original-artifacts.ts';
import {
	componentDialog,
	componentNegatives,
	componentStates,
	supportDirectory,
} from './scene-contracts.ts';
import { readJson, requireDigest, sourceDigest } from './source-binding.ts';
export interface ComponentImage {
	readonly path: string;
	readonly sha256: string;
	readonly screen: 'Quota status';
	readonly dialog: string;
	readonly journey: string;
	readonly viewport: TerminalViewport;
	readonly source_head: string;
	readonly component_sha256: string;
	readonly driver_sha256: string;
	readonly checker_sha256: string;
	readonly source_sha256: string;
	readonly result: string;
	readonly visual_inspection: 'pending';
	readonly limits: string;
}
export function componentFolderPrefix(plan: ComponentPlan): string {
	return plan.scenario === 'normal' ? 'quota-component-normal' : 'quota-component-hard-cases';
}
export function collectComponentImages(plan: ComponentPlan): ComponentImage[] {
	const expectedFolders = plan.grids.map((grid) =>
		join(
			plan.output,
			`${componentFolderPrefix(plan)}-${grid.columns}x${grid.rows}`,
			'component-receipt.json',
		),
	);
	requireExactSet(
		findReceiptPaths(plan.output, 'component-receipt.json'),
		expectedFolders,
		'missing or extra component receipt grid',
	);
	const records: ComponentImage[] = [];
	for (const grid of plan.grids) {
		const folder = join(plan.output, `${componentFolderPrefix(plan)}-${grid.columns}x${grid.rows}`);
		const receipt = componentReceiptSchema.parse(readJson(join(folder, 'component-receipt.json')));
		if (receipt.columns !== grid.columns || receipt.rows !== grid.rows)
			throw new Error('component receipt viewport mismatch');
		if (
			receipt.kind !==
			(plan.scenario === 'normal'
				? 'compiled-quota-component-normal'
				: 'compiled-quota-component-hard-cases')
		)
			throw new Error('component receipt scenario mismatch');
		if (
			receipt.node_sha256 !== plan.node.sha256 ||
			receipt.binary_sha256 !== plan.component_binary.sha256 ||
			receipt.verifier_sha256 !== plan.image_verifier.sha256
		)
			throw new Error('component receipt executable changed');
		const support = join(plan.repository, supportDirectory);
		for (const [name, digest] of new Map([
			['verify_screen_checkpoint.swift', receipt.checkpoint_source_sha256],
			['verify_screen_image.swift', receipt.recognition_source_sha256],
			['quota_component_capture.rs', receipt.driver_source_sha256],
			['quota_component_tape.rs', receipt.tape_source_sha256],
		]))
			requireDigest(join(support, name), digest);
		requireDigest(
			join(plan.repository, 'scripts/verification/tui/fixture-child.ts'),
			receipt.fixed_child_sha256,
		);
		requireDigest(join(folder, 'quota.tape'), receipt.tape_sha256);
		const scenes = componentStates(plan, grid).map((state) => `component-quota-${state}`);
		requireExactOrder(
			receipt.images.map((image) => image.checkpoint),
			scenes,
			'component scene missing, replaced, repeated or reordered',
		);
		requireExactSet(
			readdirSync(folder).filter((name) => name.endsWith('.png')),
			scenes.map((scene) => `${scene}.png`),
			'component original image census mismatch',
		);
		for (const [scene, wrong] of componentNegatives(plan, grid)) {
			const rejection = imageRejectionSchema.parse(
				readJson(join(folder, `reject-${scene}-${wrong}.image-observation.json`)),
			);
			if (rejection.checkpoint !== scene) throw new Error('component negative checkpoint mismatch');
		}
		for (const entry of receipt.images) {
			const image = join(folder, `${entry.checkpoint}.png`);
			requireDigest(image, entry.png_sha256);
			const observation = imageObservationSchema.parse(
				readJson(join(folder, `${entry.checkpoint}.image-observation.json`)),
			);
			if (observation.checkpoint !== entry.checkpoint)
				throw new Error('component image checkpoint mismatch');
			if (
				['original', 'draft', 'cancelled', 'saved', 'choice-returned'].some(
					(state) => entry.checkpoint === `component-quota-policy-${state}`,
				)
			)
				requireExactOrder(
					observation.recognizedFieldPairs,
					[
						`use credits ${entry.checkpoint.endsWith('policy-draft') || entry.checkpoint.endsWith('policy-saved') ? 'allow' : 'disallow'}`,
					],
					'component lacks associated policy value',
				);
			if (entry.checkpoint === 'component-quota-floor-saved')
				requireExactOrder(
					observation.recognizedFieldPairs,
					['current 1%'],
					'component lacks associated saved floor value',
				);
			requireOriginalPng(image);
			records.push({
				path: image,
				sha256: entry.png_sha256,
				screen: 'Quota status',
				dialog: componentDialog(entry.checkpoint.replace('component-quota-', '')),
				journey: entry.checkpoint,
				viewport: grid,
				source_head: plan.source.head,
				component_sha256: plan.component_binary.sha256,
				driver_sha256: plan.capture_driver.sha256,
				checker_sha256: plan.image_verifier.sha256,
				source_sha256: sourceDigest(plan.source),
				result:
					'Scene/receipt checks passed; personal visual and independent cleanup review pending',
				visual_inspection: 'pending',
				limits:
					'Synthetic external ports/model; real compiled UI/input/PTY. Not provider/storage/public-entry acceptance. ' +
					(entry.checkpoint === 'component-quota-confirm-help-identity'
						? 'Exact full name and tag value share the measured Account field; OCR does not prove the appearance of its one leading/trailing decoration. Personal original-image inspection must verify literal tag brackets; raw OCR is unchanged.'
						: ''),
			});
		}
	}
	return records;
}
