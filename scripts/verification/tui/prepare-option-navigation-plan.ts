/** Bind S6b's explicit target manifest to the verifier and known six-row fixture.
 * node scripts/verification/tui/prepare-option-navigation-plan.ts PRODUCT.json DRIVER.json OUTPUT CHROME
 * Prints a plan; launches no PTY and does not confer the native window.
 */
import { readFileSync, realpathSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

import { z } from 'zod';

import { isMain, reportCliFailure } from './cli-arguments.ts';
import { artifactBindingSchema, sourceBindingSchema } from './evidence-schema.ts';
import { optionNavigationPlanSchema } from './option-navigation-plan.ts';
import type { OptionNavigationPlan } from './option-navigation-plan.ts';
import {
	captureArtifact,
	fileDigest,
	sourceBinding,
	sourceDigest,
	verifyArtifact,
} from './source-binding.ts';
const targetSchema = z.object({
	sourceHead: z.string().length(40),
	sourceSnapshotPath: z.string(),
	sourceCanonicalSha256: z.string().length(64),
	sourceSnapshotFileSha256: z.string().length(64),
	consumerDiffPath: z.string(),
	consumerDiffSha256: z.string().length(64),
	product: artifactBindingSchema,
	requiredCoreHead: z.literal('bb32f10af31d27d9b35296d4ba7c6e472146d189'),
	frameMarker: z.object({ wire: z.string().min(1) }),
	fixtureSurface: z.literal('sessions-filters'),
});
const driverSchema = z.object({
	repository: z.string().min(1),
	source: sourceBindingSchema,
	sourceDigest: z.string().regex(/^[0-9a-f]{64}$/u),
	driver: artifactBindingSchema,
});
export function prepareOptionNavigation(
	args: readonly string[],
	observerRepository = process.cwd(),
): OptionNavigationPlan {
	const [manifestPath, driverPath, output, chrome] = z
		.tuple([z.string(), z.string(), z.string(), z.string()])
		.parse(args);
	const manifest = targetSchema.parse(JSON.parse(readFileSync(manifestPath, 'utf8')));
	const driver = driverSchema.parse(JSON.parse(readFileSync(driverPath, 'utf8')));
	if (sourceDigest(driver.source) !== driver.sourceDigest)
		throw new Error('driver source digest mismatch');
	verifyArtifact(driver.driver);
	verifyArtifact(manifest.product);
	for (const [path, digest] of [
		[manifest.sourceSnapshotPath, manifest.sourceSnapshotFileSha256],
		[manifest.consumerDiffPath, manifest.consumerDiffSha256],
	])
		if (path === undefined || digest === undefined || fileDigest(path) !== digest)
			throw new Error('S6b declaration file hash changed');
	const source = sourceBindingSchema.parse(
		JSON.parse(readFileSync(manifest.sourceSnapshotPath, 'utf8')),
	);
	if (
		source.head !== manifest.sourceHead ||
		sourceDigest(source) !== manifest.sourceCanonicalSha256
	)
		throw new Error('declared source digest mismatch');
	const repository = realpathSync(resolve(dirname(manifestPath), '../..'));
	const plan = optionNavigationPlanSchema.parse({
		repository,
		source,
		productSourceDigest: manifest.sourceCanonicalSha256,
		driverSource: { repository: realpathSync(driver.repository), source: driver.source },
		driverSourceDigest: driver.sourceDigest,
		observer: {
			repository: realpathSync(observerRepository),
			source: sourceBinding(observerRepository),
		},
		requiredCrateHead: manifest.requiredCoreHead,
		product: manifest.product,
		driver: driver.driver,
		node: captureArtifact(process.execPath),
		chrome: realpathSync(chrome),
		output: resolve(output),
		rows: [
			{ label: 'Codex Alpha', visiblePrefix: 'Codex A' },
			{ label: 'Codex Beta', visiblePrefix: 'Codex B' },
			{ label: 'Claude Gamma', visiblePrefix: 'Claude G' },
			{ label: 'Claude Delta', visiblePrefix: 'Claude D' },
			{ label: 'Cursor Echo', visiblePrefix: 'Cursor E' },
			{ label: 'Claude-lookalike Zeta', visiblePrefix: 'Claude-' },
		],
	});
	return plan;
}
if (isMain(import.meta.url))
	try {
		process.stdout.write(
			JSON.stringify(prepareOptionNavigation(process.argv.slice(2)), null, 2) + '\n',
		);
	} catch (error) {
		reportCliFailure(error);
	}
