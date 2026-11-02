import { isMain, planArgument, reportCliFailure, strictArguments } from '../cli/cli-arguments.ts';
import { readJson } from '../evidence/source-binding.ts';
import { publicPlanSchema, runPublicCapture, checkPublicCapture } from './public-sessions.ts';
export async function publicSessionsMain(args: readonly string[]): Promise<void> {
	const { values, positionals } = strictArguments(
		args,
		{ help: { type: 'boolean' }, check: { type: 'boolean' } },
		true,
	);
	if (values.help) {
		process.stdout.write('public-sessions-run.ts PLAN.json [--check]\n');
		return;
	}
	const file = planArgument(positionals);
	const plan = publicPlanSchema.parse(readJson(file));
	if (values.check) await checkPublicCapture(plan);
	else await runPublicCapture(plan);
}
if (isMain(import.meta.url))
	await publicSessionsMain(process.argv.slice(2)).catch(reportCliFailure);
