import { isMain, planArgument, reportCliFailure, strictArguments } from './cli-arguments.ts';
import { publicPlanSchema, runPublicCapture } from './public-sessions.ts';
import { readJson } from './source-binding.ts';
export async function publicSessionsMain(args: readonly string[]): Promise<void> {
	const { values, positionals } = strictArguments(args, { help: { type: 'boolean' } }, true);
	if (values.help) {
		process.stdout.write('public-sessions-run.ts PLAN.json\n');
		return;
	}
	const file = planArgument(positionals);
	await runPublicCapture(publicPlanSchema.parse(readJson(file)));
}
if (isMain(import.meta.url))
	await publicSessionsMain(process.argv.slice(2)).catch(reportCliFailure);
