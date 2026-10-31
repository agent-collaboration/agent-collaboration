/** Authored future journeys and offline oracle checks; this entry never launches a TUI.
 * node scripts/verification/tui/s9-prestage.ts --check | --list
 * Entries stay absent-at-base until their S6/S7 target and fixture seams are declared.
 */
import { isMain, reportCliFailure, strictArguments, usageFailure } from './cli/cli-arguments.ts';
import { s9JourneySchema, s9DesignHead } from './s9-journey-definition.ts';
import type { S9Journey } from './s9-journey-definition.ts';
import { accountActionJourneys } from './s9-journeys/account-actions.ts';
import { accountLoginJourneys } from './s9-journeys/account-login.ts';
import { claudeDetailsJourneys } from './s9-journeys/claude-details.ts';
import { newDialogJourneys } from './s9-journeys/new-dialog.ts';
import { quotaRefreshJourneys } from './s9-journeys/quota-refresh.ts';
import { toolbarMenuJourneys } from './s9-journeys/toolbar-menus.ts';
export const s9AuthoredJourneys: readonly S9Journey[] = [
	...toolbarMenuJourneys,
	...claudeDetailsJourneys,
	...newDialogJourneys,
	...accountLoginJourneys,
	...accountActionJourneys,
	...quotaRefreshJourneys,
];
export function validateS9Journeys(): void {
	const ids = new Set<string>();
	for (const candidate of s9AuthoredJourneys) {
		const journey = s9JourneySchema.parse(candidate);
		if (ids.has(journey.journeyId)) throw new Error('duplicate S9 journey ' + journey.journeyId);
		ids.add(journey.journeyId);
		const checkpoints = journey.steps
			.filter((step) => step.kind === 'capture')
			.map((step) => step.checkpoint);
		if (new Set(checkpoints).size !== checkpoints.length)
			throw new Error('duplicate checkpoint ' + journey.journeyId);
	}
	for (const requirement of ['U12/R13', 'U5/R9', 'U16/R17', 'U17/R18'])
		for (const keys of ['legacy', 'kitty'])
			if (
				!s9AuthoredJourneys.some(
					(journey) =>
						journey.requirements.some((row) => row === requirement) &&
						journey.keys.some((mode) => mode === keys),
				)
			)
				throw new Error('missing requirement/key mode ' + requirement + '/' + keys);
}
export function s9PrestageMain(args: readonly string[]): void {
	const { values } = strictArguments(
		args,
		{ help: { type: 'boolean' }, check: { type: 'boolean' }, list: { type: 'boolean' } },
		false,
	);
	if (values.help) {
		process.stdout.write(
			's9-prestage.ts --check | --list: validate authored S9 contracts and list absent-at-base entries; no product observation or launch.\n',
		);
		return;
	}
	if ([values.check, values.list].filter(Boolean).length !== 1)
		usageFailure('choose --check or --list');
	validateS9Journeys();
	process.stdout.write(
		JSON.stringify(
			{
				kind: 'authored-not-driven',
				design: s9DesignHead,
				journeys: s9AuthoredJourneys.map((journey) => ({
					journeyId: journey.journeyId,
					requirements: journey.requirements,
					availability: journey.availability,
					grids: journey.grids,
					keys: journey.keys,
					fixtures: journey.fixtures,
				})),
				admissionInputs: s9AuthoredJourneys.flatMap((journey) => journey.admissionInputs),
				productPasses: 0,
			},
			null,
			2,
		) + '\n',
	);
}
if (isMain(import.meta.url)) {
	try {
		s9PrestageMain(process.argv.slice(2));
	} catch (error) {
		reportCliFailure(error);
	}
}
