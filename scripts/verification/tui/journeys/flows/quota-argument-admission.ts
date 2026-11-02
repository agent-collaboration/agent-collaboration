/** Offline argument profiles. A source check never admits an absent executable.
 * Native execution still needs the exact signed driver/product declaration and a window.
 */
import { z } from 'zod';

import { canonicalJson } from '../../evidence/source-binding.ts';
import { COMPONENT_SCENARIOS, COMPONENT_TEST } from '../../fixtures/component-fixture-contract.ts';
import { shortTemporaryRoot } from '../../fixtures/short-temporary-root.ts';
import { quotaEffectArguments, quotaEffectScenarioSchema } from './quota-fixture-contract.ts';

const unboundArtifactSchema = z.object({
	artifact: z.null(),
	argvSuffix: z.array(z.string()),
	scenarioKeys: z.array(z.string()),
});
export const quotaOfflinePacketSchema = z.object({
	worktree: z.string().startsWith('/'),
	sourceHead: z.string().regex(/^[a-f0-9]{40}$/u),
	sourceFrozen: z.literal(false),
	nativeProductLaunchAuthorized: z.literal(false),
	ownedSourceManifest: z.string().min(1),
	ownedSourceManifestSha256: z.string().regex(/^[a-f0-9]{64}$/u),
	sourceFilesSnapshot: z.string().min(1),
	sourceSnapshotRetained: z.literal(true),
	effectful: unboundArtifactSchema.extend({
		package: z.literal('codex-router-cli'),
		features: z.tuple([z.literal('quota-reset-test-harness')]),
		configurationEnvironment: z.literal('QUOTA_EFFECT_FIXTURE_CONFIG'),
		configurationIs: z.literal('path to JSON file, not inline JSON'),
		configurationFields: z.array(z.string()),
		loginEnvironment: z.array(z.string()),
		loginExit: z.strictObject({
			'login-succeeded': z.literal(0),
			'login-cancelled130': z.literal(130),
			'login-failed7': z.literal(7),
		}),
	}),
	readOnly: unboundArtifactSchema.extend({
		package: z.literal('agent-router-quota-tui'),
		scenarioEnvironment: z.literal('QUOTA_COMPONENT_FIXTURE'),
		optionalReceiptEnvironment: z.array(z.string()),
		releaseEnvironment: z.literal('QUOTA_COMPONENT_FIXTURE_RELEASE_FILE'),
		releaseRequiredFor: z.array(z.string()),
		releaseBytes: z.literal('settle\n'),
	}),
	commonChildEnvironment: z.strictObject({
		NO_COLOR: z.literal('absent'),
		TMPDIR: z.literal(shortTemporaryRoot()),
	}),
	r19: z.object({ status: z.string().startsWith('unbound placeholder') }),
});
export type QuotaOfflinePacket = z.infer<typeof quotaOfflinePacketSchema>;
const readOnlyArguments = [
	'--exact',
	COMPONENT_TEST,
	'--ignored',
	'--nocapture',
	'--test-threads=1',
];
const pendingReadOnlyScenarios = [
	'inspection-pending',
	'commit-pending',
	'revalidation-pending',
	'refresh-pending',
	'save-pending',
];

export function admitQuotaArgumentProfile(options: {
	readonly kind: 'effectful' | 'read-only';
	readonly scenario: string;
	readonly arguments: readonly string[];
}): void {
	const expected = options.kind === 'effectful' ? quotaEffectArguments : readOnlyArguments;
	if (canonicalJson(options.arguments) !== canonicalJson(expected))
		throw new Error('quota fixture argv differs from the exact declared entry');
	if (options.kind === 'effectful') quotaEffectScenarioSchema.parse(options.scenario);
	else if (!Object.hasOwn(COMPONENT_SCENARIOS, options.scenario))
		throw new Error('unknown read-only quota scenario');
}

export function quotaArgumentAdmission(packetValue: unknown): unknown {
	const packet = quotaOfflinePacketSchema.parse(packetValue);
	for (const [actual, expected] of [
		[packet.effectful.scenarioKeys, quotaEffectScenarioSchema.options],
		[packet.readOnly.scenarioKeys, Object.keys(COMPONENT_SCENARIOS)],
		[packet.readOnly.releaseRequiredFor, pendingReadOnlyScenarios],
		[
			packet.effectful.configurationFields,
			[
				'scenario',
				'outputDirectory',
				'events',
				'receipt',
				'controlSocket',
				'dispatchReceipt',
				'loginReceipt',
				'loginGateSocket',
				'loginStandin',
			],
		],
		[
			packet.effectful.loginEnvironment,
			[
				'QUOTA_LOGIN_STANDIN_RECEIPT',
				'QUOTA_LOGIN_STANDIN_GATE',
				'QUOTA_LOGIN_STANDIN_EXIT',
				'PATH',
			],
		],
		[
			packet.readOnly.optionalReceiptEnvironment,
			['QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE', 'QUOTA_COMPONENT_FIXTURE_DISPATCH_RECEIPT_FILE'],
		],
	] as const)
		if (canonicalJson([...actual].toSorted()) !== canonicalJson([...expected].toSorted()))
			throw new Error('quota packet scenario/environment census changed');
	const bindings = (['effectful', 'read-only'] as const).flatMap((kind) => {
		const source = kind === 'effectful' ? packet.effectful : packet.readOnly;
		return source.scenarioKeys.flatMap((scenario) => {
			admitQuotaArgumentProfile({ kind, scenario, arguments: source.argvSuffix });
			const routes =
				kind === 'effectful' && scenario.startsWith('refresh-') ? ['key', 'menu'] : ['authored'];
			return routes.flatMap((route) =>
				(['legacy', 'kitty'] as const).map((keys) => ({
					kind,
					scenario,
					route,
					keys,
					arguments: source.argvSuffix,
					configuration:
						kind === 'effectful'
							? {
									variable: packet.effectful.configurationEnvironment,
									storage: 'fresh private JSON configuration, not inline JSON',
									outputPaths: `distinct absent direct children of an owned mode-0700 directory under ${shortTemporaryRoot()}`,
									login: scenario.startsWith('login-')
										? 'declared signed stand-in plus owned PATH prefix and causal gate'
										: scenario === 'grouping-reopen'
											? 'same private stand-in guard; no child or gate listener'
											: 'login fields forbidden',
								}
							: {
									variable: packet.readOnly.scenarioEnvironment,
									value: scenario,
									release: pendingReadOnlyScenarios.includes(scenario)
										? {
												variable: packet.readOnly.releaseEnvironment,
												bytes: packet.readOnly.releaseBytes,
											}
										: null,
								},
					sourceArgumentProfile: 'matches',
					executableAdmission: {
						kind: 'unbound',
						reason: 'S7 artifact is null; no executable argument check attempted',
					},
				})),
			);
		});
	});
	return {
		kind: 'quota-offline-argument-admission',
		productLaunches: 0,
		productPasses: 0,
		sourceProfiles: bindings.length,
		executableChecks: 0,
		bindings,
		r19: { kind: 'unbound', reason: packet.r19.status },
	};
}
