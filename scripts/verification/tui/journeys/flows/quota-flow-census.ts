/** Source-backed quota B preparation; never starts a product.
 * node scripts/verification/tui/journeys/flows/quota-flow-census.ts REPOSITORY PACKET.json OUTPUT.json
 * Pending artifacts and native adapters stay named gaps, never admitted product cases.
 */
import { execFileSync } from 'node:child_process';
import { readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { z } from 'zod';

import {
	isMain,
	reportCliFailure,
	strictArguments,
	usageFailure,
} from '../../cli/cli-arguments.ts';
import { captureArtifact, canonicalJson } from '../../evidence/source-binding.ts';
import { journeyGridSizes } from '../journey-definition.ts';
import { authoredFlows, pendingGroupingFlows } from './flow-catalogue.ts';
import { quotaArgumentAdmission, quotaOfflinePacketSchema } from './quota-argument-admission.ts';
import {
	quotaEffectArguments,
	quotaEffectScenarioSchema,
	quotaJourneyScenarios,
} from './quota-fixture-contract.ts';

const quotaSourceRules = {
	'crates/codex-router-cli/src/quota/quota_effectful_fixture/configuration.rs': [
		'pub(super) enum Scenario',
		'pub(super) struct FixtureConfiguration',
		'pub(super) enum FixtureControlRequest',
	],
	'crates/codex-router-cli/src/quota/quota_effectful_fixture/mod.rs': [
		'async fn compiled_quota_effectful_fixture()',
		'"nativeDispatch"',
		'"fixture-finished"',
	],
	'crates/codex-router-cli/src/quota/quota_effectful_fixture/owned_state.rs': [
		'format!("effect-account-{index}")',
		'with_active_credential_generation(7)',
	],
	'crates/codex-router-cli/src/quota/quota_effectful_fixture/receipts.rs': [
		'"fixture-control-ack"',
		'"mapping-returned"',
	],
	'crates/codex-router-cli/src/quota/quota_effectful_fixture/control_server.rs': [
		'"fixture-gate-released"',
		'"actualInputSequenceThrough"',
		'"login-child-entered"',
	],
	'crates/codex-router-cli/src/quota/quota_effectful_fixture/validation.rs': [
		'"actual command retargeted or ran in the wrong sequence"',
		'"fixture did not terminate through the actual guarded Quit"',
	],
	'crates/codex-router-cli/src/quota/quota_effectful_fixture/refresh_fixture.rs': [
		'"refresh-first-request-held"',
		'"refresh-setup-held"',
		'"serviceCalls"',
	],
	'crates/agent-router-quota-tui/src/quota_group_controls.rs': [
		'"group-cycle"',
		'&["F6"]',
		'crate::QuotaGroupFacet::Provider',
		'crate::QuotaGroupFacet::Status',
	],
	'crates/codex-router-cli/src/quota/quota_account_adapter_test.rs': ['mod effectful_fixture;'],
	'crates/agent-router-quota-tui/src/quota_storyboard_fixture_test.rs': [
		'enum FixtureScenario',
		'fn from_environment()',
		'async fn compiled_quota_component_storyboard_fixture()',
	],
} satisfies Readonly<Record<string, readonly string[]>>;

/** Parse only the source enum named by the producer; a new variant requires an explicit binding. */
export function quotaScenarioNames(configuration: string): readonly string[] {
	const body = /pub\(super\) enum Scenario\s*\{([^}]+)\}/u.exec(configuration)?.[1];
	if (body === undefined) throw new Error('S7 Scenario enum absent');
	return body
		.split(',')
		.map((name) => name.trim())
		.filter(Boolean)
		.map((name) =>
			name.replace(/[A-Z]/gu, (letter, index) => (index === 0 ? '' : '-') + letter.toLowerCase()),
		);
}

export function quotaFlowCensus(repository: string, packetPath: string): unknown {
	const root = realpathSync(repository);
	const packet = quotaOfflinePacketSchema.parse(JSON.parse(readFileSync(packetPath, 'utf8')));
	const packetBinding = captureArtifact(packetPath);
	if (realpathSync(packet.worktree) !== root)
		throw new Error('S7 packet worktree differs from source');
	const sourceRoot = realpathSync(resolve(root, packet.sourceFilesSnapshot));
	if (!sourceRoot.startsWith(root + '/'))
		throw new Error('S7 source snapshot leaves declared worktree');
	const manifestPath = resolve(root, packet.ownedSourceManifest);
	if (
		!manifestPath.startsWith(root + '/') ||
		captureArtifact(manifestPath).sha256 !== packet.ownedSourceManifestSha256
	)
		throw new Error('S7 source manifest changed');
	const manifest = z
		.object({
			files: z.array(z.object({ path: z.string(), sha256: z.string().regex(/^[a-f0-9]{64}$/u) })),
		})
		.parse(JSON.parse(readFileSync(manifestPath, 'utf8')));
	for (const file of manifest.files) {
		const path = resolve(sourceRoot, file.path);
		if (!path.startsWith(sourceRoot + '/') || captureArtifact(path).sha256 !== file.sha256)
			throw new Error('S7 packet source changed: ' + file.path);
	}
	const sources = Object.entries(quotaSourceRules).map(([path, rules]) => {
		const absolute = resolve(sourceRoot, path),
			lines = readFileSync(absolute, 'utf8').split('\n');
		return Object.assign({}, captureArtifact(absolute), {
			rules: rules.map((text) => {
				const line = lines.findIndex((value) => value.includes(text));
				if (line < 0) throw new Error(`source rule missing: ${path}: ${text}`);
				return { line: line + 1, text };
			}),
		});
	});
	const configuration = readFileSync(
		resolve(
			sourceRoot,
			'crates/codex-router-cli/src/quota/quota_effectful_fixture/configuration.rs',
		),
		'utf8',
	);
	const scenarios = quotaScenarioNames(configuration).map((name) =>
		quotaEffectScenarioSchema.parse(name),
	);
	if (
		canonicalJson(scenarios.toSorted()) !==
		canonicalJson([...quotaEffectScenarioSchema.options].toSorted())
	)
		throw new Error('S7 scenario set changed; stop and rebind');
	const readOnlySource = readFileSync(
		resolve(sourceRoot, 'crates/agent-router-quota-tui/src/quota_storyboard_fixture_test.rs'),
		'utf8',
	);
	const readOnlyScenarios = Array.from(
		readOnlySource.matchAll(/Ok\("([a-z-]+)"\)\s*=>/gu),
		(match) => z.string().parse(match[1]),
	);
	if (
		canonicalJson(readOnlyScenarios.toSorted()) !==
		canonicalJson([...packet.readOnly.scenarioKeys].toSorted())
	)
		throw new Error('S7 read-only source scenario set changed');
	const flows = authoredFlows
		.filter((flow) => flow.surface === 'quota')
		.map((flow) => ({
			journeyId: flow.journeyId,
			grids: flow.grids,
			keys: flow.keys,
			cases: flow.grids.length * flow.keys.length,
			scenarios: quotaJourneyScenarios(flow.journeyId),
			requiredFixtures: flow.steps
				.filter((step) => step.kind === 'fixture')
				.map((step) => step.operation),
			requiredChecks: [
				...new Set(
					flow.steps.flatMap((step) =>
						step.kind === 'capture' || step.kind === 'readback' ? step.checks : [],
					),
				),
			],
			nativeAdapter: 'not-bound',
		}));
	const covered = new Set(flows.flatMap((flow) => flow.scenarios));
	return {
		kind: 'quota-b-source-preparation',
		productPasses: 0,
		productLaunches: 0,
		packet: packetBinding,
		verifiedManifestFiles: manifest.files.length,
		sourceRepository: root,
		sourceRoot,
		sourceHead: packet.sourceHead,
		liveHeadAtCensus: execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], {
			encoding: 'utf8',
		}).trim(),
		sourceQualification: 'retained selected-file source snapshot, not a frozen product declaration',
		sources,
		effectful: {
			arguments: quotaEffectArguments,
			configurationVariable: 'QUOTA_EFFECT_FIXTURE_CONFIG',
			scenarios: scenarios.map((scenario) => ({
				scenario,
				catalogue: covered.has(scenario) ? 'mapped' : 'missing-authored-journey',
				routes: scenario.startsWith('refresh-') ? ['key', 'menu'] : ['authored'],
			})),
		},
		readOnly: { scenarios: readOnlyScenarios, fixtureEnvironment: 'QUOTA_COMPONENT_FIXTURE' },
		flows,
		pendingGrouping: pendingGroupingFlows.filter((flow) => flow.surface === 'quota'),
		heldRemoval: {
			journeyId: 'quota-remove-middle',
			additionalScenario: 'remove-held',
			grids: journeyGridSizes,
			keys: ['legacy', 'kitty'],
			cases: 8,
			state: 'causal release and retained completion binding required',
		},
		argumentAdmission: quotaArgumentAdmission(packet),
		gaps: [
			'S7 frozen signed effectful libtest, read-only artifacts and login stand-in declaration pending',
			'effectful and all-11 read-only streaming-driver source is prepared; rebuilt signed driver not qualified',
			'quota native flow input/focus/pointer bindings and active-surface ACK expectations not yet wired',
			'refresh key/menu legs require separate fresh children; current authored parent has no child transition binding',
			'held removal variant of the middle-removal flow still needs causal gate release and retained completion binding',
			'R19 quota grouping remains a placeholder until declaration',
		],
	};
}
if (isMain(import.meta.url)) {
	try {
		const { values, positionals } = strictArguments(
			process.argv.slice(2),
			{ help: { type: 'boolean' } },
			true,
		);
		if (values.help)
			process.stdout.write(
				'quota-flow-census.ts REPOSITORY PACKET.json OUTPUT.json: inspect S7 source and enumerate every quota B contract and admission gap; launches no product.\n',
			);
		else {
			const [repository, packet, output, ...extra] = positionals;
			if (
				repository === undefined ||
				packet === undefined ||
				output === undefined ||
				extra.length > 0
			)
				usageFailure('repository, S7 packet and fresh output file required');
			writeFileSync(output, JSON.stringify(quotaFlowCensus(repository, packet), null, 2) + '\n', {
				flag: 'wx',
			});
		}
	} catch (error) {
		reportCliFailure(error);
	}
}
