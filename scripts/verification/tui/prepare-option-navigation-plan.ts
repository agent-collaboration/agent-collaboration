/** Bind S6b's explicit target manifest to the verifier and known six-row fixture.
 * node scripts/verification/tui/prepare-option-navigation-plan.ts TARGET.json OUTPUT CHROME
 * Prints a plan; launches no PTY and does not confer the native window.
 */
import { readFileSync, realpathSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

import { z } from 'zod';

import { isMain, reportCliFailure } from './cli-arguments.ts';
import { artifactBindingSchema, sourceBindingSchema } from './evidence-schema.ts';
import { optionNavigationPlanSchema } from './option-navigation-plan.ts';
import { captureArtifact, fileDigest, sourceBinding, sourceDigest } from './source-binding.ts';
const targetSchema = z.object({
	sourceHead: z.string().length(40),
	sourceSnapshotPath: z.string(),
	sourceCanonicalSha256: z.string().length(64),
	sourceSnapshotFileSha256: z.string().length(64),
	consumerDiffPath: z.string(),
	consumerDiffSha256: z.string().length(64),
	product: artifactBindingSchema,
	driver: artifactBindingSchema,
	nodeVersion: z.literal('v26.12.1'),
	fixtureSurface: z.literal('sessions-filters'),
});
export function prepareOptionNavigation(args: readonly string[]): void {
	const [manifestPath, output, chrome] = z.tuple([z.string(), z.string(), z.string()]).parse(args);
	const manifest = targetSchema.parse(JSON.parse(readFileSync(manifestPath, 'utf8')));
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
	const observerRepository = realpathSync(process.cwd());
	const plan = optionNavigationPlanSchema.parse({
		repository,
		source,
		observer: { repository: observerRepository, source: sourceBinding(observerRepository) },
		requiredCrateHead: '76d02263fc9e3632839a725442b893ec5dcd77cf',
		product: manifest.product,
		driver: manifest.driver,
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
	process.stdout.write(JSON.stringify(plan, null, 2) + '\n');
}
if (isMain(import.meta.url))
	try {
		prepareOptionNavigation(process.argv.slice(2));
	} catch (error) {
		reportCliFailure(error);
	}
