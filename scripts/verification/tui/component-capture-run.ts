import { runComponentCapture } from './capture-execution.ts';
import { isMain, planArgument, reportCliFailure, strictArguments } from './cli-arguments.ts';
import { componentPlanSchema } from './evidence-schema.ts';
import { readJson } from './source-binding.ts';
export async function componentCaptureMain(args: readonly string[]): Promise<void> {
	const { values, positionals } = strictArguments(args, { help: { type: 'boolean' } }, true);
	if (values.help) {
		process.stdout.write('component-capture-run.ts PLAN.json\n');
		return;
	}
	const file = planArgument(positionals);
	await runComponentCapture(componentPlanSchema.parse(readJson(file)));
}
if (isMain(import.meta.url))
	await componentCaptureMain(process.argv.slice(2)).catch(reportCliFailure);
