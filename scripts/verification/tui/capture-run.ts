import { checkDriver, runCapture } from './capture-execution.ts';
import type { CaptureSelection } from './capture-selection.ts';
import {
	isMain,
	planArgument,
	reportCliFailure,
	strictArguments,
	usageFailure,
} from './cli-arguments.ts';
import { capturePlanSchema, fixtureScenarioSchema } from './evidence-schema.ts';
import { readJson } from './source-binding.ts';
export async function captureMain(args: readonly string[]): Promise<void> {
	const { values, positionals } = strictArguments(
		args,
		{
			help: { type: 'boolean' },
			check: { type: 'boolean' },
			'quota-only': { type: 'boolean' },
			'sessions-only': { type: 'boolean' },
			'cua-surface': { type: 'string' },
			'cua-quota-fixture': { type: 'string' },
		},
		true,
	);
	if (values.help) {
		process.stdout.write(
			'capture-run.ts PLAN.json [--check] [--quota-only | --sessions-only | --cua-surface sessions|quota] [--cua-quota-fixture SCENARIO]\n',
		);
		return;
	}
	if (
		[values['quota-only'], values['sessions-only'], values['cua-surface'] !== undefined].filter(
			Boolean,
		).length > 1
	)
		usageFailure('surface selection not allowed with argument from another mode');
	const planFile = planArgument(positionals);
	const surface = values['cua-surface'];
	if (surface !== undefined && surface !== 'sessions' && surface !== 'quota')
		usageFailure('invalid CUA surface');
	const selection: CaptureSelection = values['quota-only']
		? 'static-quota'
		: values['sessions-only']
			? 'static-sessions'
			: surface === 'quota'
				? 'cua-quota'
				: surface === 'sessions'
					? 'cua-sessions'
					: 'static-both';
	const fixtureValue = values['cua-quota-fixture'];
	if (fixtureValue !== undefined && surface !== 'quota')
		usageFailure('--cua-quota-fixture requires --cua-surface quota');
	const parsedFixture =
		fixtureValue === undefined ? null : fixtureScenarioSchema.safeParse(fixtureValue);
	if (parsedFixture !== null && !parsedFixture.success) usageFailure('invalid CUA quota fixture');
	const fixture = parsedFixture === null ? null : parsedFixture.data;
	const plan = capturePlanSchema.parse(readJson(planFile));
	if (values.check) {
		await checkDriver(plan, selection, fixture);
		process.stdout.write(
			'target binding, exact capture test and blank-image rejection verified; no TUI observation\n',
		);
	} else {
		await runCapture(plan, selection, fixture);
		process.stdout.write(
			`${plan.output}/${surface === undefined ? 'gallery-manifest.json' : 'cua-lifecycle-summary.json'}\n`,
		);
	}
}
if (isMain(import.meta.url)) await captureMain(process.argv.slice(2)).catch(reportCliFailure);
