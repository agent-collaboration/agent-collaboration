import { readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';

import type { CaptureSelection } from './capture-selection.ts';
import { staticSurfaces, staticViewports } from './capture-selection.ts';
import {
	filterReceiptSchema,
	imageObservationSchema,
	imageRejectionSchema,
	requireSelectedChoice,
} from './evidence-schema.ts';
import type { CapturePlan } from './evidence-schema.ts';
import {
	findReceiptPaths,
	requireExactOrder,
	requireExactSet,
	requireOriginalPng,
} from './original-artifacts.ts';
import {
	checkpointDialog,
	proofLimits,
	quotaCheckpointStates,
	quotaNegativePairs,
	sessionCheckpointStates,
	sessionDetailsFields,
	sessionDetailsPoses,
	sessionDetailsRequiredFields,
	supportDirectory,
} from './scene-contracts.ts';
import { readJson, requireDigest, sourceDigest } from './source-binding.ts';
export interface OriginalImage {
	readonly path: string;
	readonly sha256: string;
	readonly screen: string;
	readonly dialog: string;
	readonly columns: number;
	readonly rows: number;
	readonly journey: string;
	readonly source_head: string;
	readonly binary_sha256: string;
	readonly capture_driver_sha256: string;
	readonly source_sha256: string;
	readonly result: 'scene-oracle-passed';
	readonly visual_inspection: 'pending';
	readonly limits: string;
}
export function collectImages(
	plan: CapturePlan,
	selection: CaptureSelection = 'static-both',
): OriginalImage[] {
	const images: OriginalImage[] = [];
	const support = join(plan.repository, supportDirectory);
	const expectedReceipts = staticSurfaces(selection).flatMap((surface) =>
		staticViewports(selection).map((grid) =>
			join(plan.output, `${surface}-compact-${grid.columns}x${grid.rows}`, 'filter-receipt.json'),
		),
	);
	requireExactSet(
		findReceiptPaths(plan.output, 'filter-receipt.json'),
		expectedReceipts,
		'capture receipt set mismatch: missing or extra surface/grid',
	);
	for (const surface of staticSurfaces(selection))
		for (const grid of staticViewports(selection)) {
			const folder = join(plan.output, `${surface}-compact-${grid.columns}x${grid.rows}`);
			const receipt = filterReceiptSchema.parse(readJson(join(folder, 'filter-receipt.json')));
			if (
				receipt.surface !== surface ||
				receipt.columns !== grid.columns ||
				receipt.rows !== grid.rows
			)
				throw new Error('receipt surface/viewport mismatch');
			for (const [actual, expected] of [
				[receipt.node_sha256, plan.node.sha256],
				[receipt.binary_sha256, plan.harness_binary.sha256],
				[receipt.vhs_sha256, plan.vhs.sha256],
				[receipt.ttyd_sha256, plan.ttyd.sha256],
				[receipt.verifier_sha256, plan.image_verifier.sha256],
			])
				if (actual !== expected) throw new Error('receipt build/tool identity mismatch');
			for (const [name, digest] of new Map([
				['provider_filter_capture.rs', receipt.driver_source_sha256],
				['provider_filter_tape.rs', receipt.tape_driver_source_sha256],
				['verify_screen_checkpoint.swift', receipt.verifier_checkpoint_source_sha256],
				['verify_screen_image.swift', receipt.verifier_source_sha256],
			]))
				requireDigest(join(support, name), digest);
			requireDigest(join(folder, 'quota.tape'), receipt.tape_sha256);
			requireDigest(
				join(plan.repository, 'scripts/verification/tui/fixture-child.ts'),
				receipt.fixed_child_sha256,
			);
			const states =
				surface === 'sessions' ? sessionCheckpointStates(grid) : quotaCheckpointStates(grid.rows);
			const expectedOrder = states.map((state) => `compact-${surface}-${state}`);
			requireExactOrder(
				receipt.images.map((image) => image.checkpoint),
				expectedOrder,
				'checkpoint sequence mismatch: missing, wrong or reordered scene',
			);
			requireExactSet(
				readdirSync(folder).filter((name) => name.endsWith('.png')),
				expectedOrder.map((name) => `${name}.png`),
				'original PNG set mismatch: missing or extra scene',
			);
			const negatives: [string, string][] = [
				[`compact-${surface}-claude`, 'blank-startup'],
				[`compact-${surface}-claude`, `compact-${surface}-all`],
				[`compact-${surface}-cancelled`, `compact-${surface}-chooser`],
			];
			if (surface === 'quota')
				for (const [expected, wrong] of quotaNegativePairs)
					negatives.push([`compact-quota-${expected}`, `compact-quota-${wrong}`]);
			if (surface === 'sessions') {
				for (const pose of sessionDetailsPoses(grid))
					negatives.push([`compact-sessions-${pose.name}`, 'compact-sessions-codex-details']);
				if (grid.columns === 48 && grid.rows === 12)
					negatives.push(['compact-sessions-details-identity', 'compact-sessions-details-title']);
			}
			for (const [expected, wrong] of negatives) {
				const rejection = imageRejectionSchema.parse(
					readJson(join(folder, `reject-${expected}-${wrong}.image-observation.json`)),
				);
				if (rejection.checkpoint !== expected) throw new Error('wrong negative-control checkpoint');
			}
			const detailsFields = new Set<string>();
			for (const entry of receipt.images) {
				const png = join(folder, `${entry.checkpoint}.png`);
				requireDigest(png, entry.png_sha256);
				const observation = imageObservationSchema.parse(
					readJson(join(folder, `${entry.checkpoint}.image-observation.json`)),
				);
				if (observation.checkpoint !== entry.checkpoint)
					throw new Error('wrong image observation checkpoint');
				if (entry.checkpoint === 'compact-quota-disabled-choice')
					requireSelectedChoice(observation, 'shown');
				if (entry.checkpoint === 'compact-quota-hidden-choice')
					requireSelectedChoice(observation, 'hidden');
				if (entry.checkpoint === 'compact-quota-chooser')
					requireExactOrder(
						observation.recognizedFieldPairs,
						['account provider openai', 'disabled accounts shown'],
						'chooser lacks exact associated uncommitted field values',
					);
				if (entry.checkpoint === 'compact-sessions-chooser')
					requireExactOrder(
						observation.recognizedFieldPairs,
						['client codex'],
						'chooser lacks exact associated uncommitted client',
					);
				if (surface === 'sessions' && entry.checkpoint.startsWith('compact-sessions-details')) {
					const fields = new Set(observation.recognizedFieldPairs);
					if (!sessionDetailsRequiredFields(entry.checkpoint).every((field) => fields.has(field)))
						throw new Error('Details lacks associated captured identity or complete field value');
					for (const field of fields) detailsFields.add(field);
				}
				requireOriginalPng(png);
				images.push({
					path: resolve(png),
					sha256: entry.png_sha256,
					screen: surface,
					dialog: checkpointDialog(entry.checkpoint.replace(`compact-${surface}-`, '')),
					columns: grid.columns,
					rows: grid.rows,
					journey: entry.checkpoint,
					source_head: plan.source.head,
					binary_sha256: receipt.binary_sha256,
					capture_driver_sha256: plan.capture_driver.sha256,
					source_sha256: sourceDigest(plan.source),
					result: 'scene-oracle-passed',
					visual_inspection: 'pending',
					limits: proofLimits,
				});
			}
			if (
				surface === 'sessions' &&
				!sessionDetailsFields.every((field) => detailsFields.has(field))
			)
				throw new Error('Details pose set lacks full captured title/path/ID/endpoint/state');
		}
	return images;
}
