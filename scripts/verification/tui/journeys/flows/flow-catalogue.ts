import {
	layerCKeyboardKeys,
	layerCProfiles,
	layerCPointerJourneys,
} from '../../capture/harness-journeys.ts';
/** Authored future journeys and offline oracle checks; this entry never launches a TUI.
 * node scripts/verification/tui/journeys/flows/flow-catalogue.ts --check | --list
 * Entries stay absent-at-base until their S6/S7 target and fixture seams are declared.
 */
import {
	isMain,
	reportCliFailure,
	strictArguments,
	usageFailure,
} from '../../cli/cli-arguments.ts';
import { accountActionJourneys } from './account-actions.ts';
import { accountLoginJourneys } from './account-login.ts';
import { claudeDetailsJourneys } from './claude-details.ts';
import { flowJourneySchema, flowDesignHead } from './flow-definition.ts';
import type { FlowJourney } from './flow-definition.ts';
import { newDialogJourneys } from './new-dialog.ts';
import { optionNavigationJourneys } from './option-navigation.ts';
import { quotaRefreshJourneys } from './quota-refresh.ts';
import { sourceQualifiedJourneys } from './source-qualified-selection.ts';
import { toolbarMenuJourneys } from './toolbar-menus.ts';
export const authoredFlows: readonly FlowJourney[] = [
	...toolbarMenuJourneys,
	...claudeDetailsJourneys,
	...sourceQualifiedJourneys,
	...newDialogJourneys,
	...accountLoginJourneys,
	...accountActionJourneys,
	...quotaRefreshJourneys,
	...optionNavigationJourneys,
];
export function validateAuthoredFlows(): void {
	const ids = new Set<string>();
	for (const candidate of authoredFlows) {
		const journey = flowJourneySchema.parse(candidate);
		if (ids.has(journey.journeyId)) throw new Error('duplicate S9 journey ' + journey.journeyId);
		ids.add(journey.journeyId);
		const checkpoints = journey.steps
			.filter((step) => step.kind === 'capture')
			.map((step) => step.checkpoint);
		if (new Set(checkpoints).size !== checkpoints.length)
			throw new Error('duplicate checkpoint ' + journey.journeyId);
	}
	for (const requirement of ['U4/R4', 'U12/R13', 'U5/R9', 'U16/R17', 'U17/R18'])
		for (const keys of ['legacy', 'kitty'])
			if (
				!authoredFlows.some(
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
	validateAuthoredFlows();
	process.stdout.write(
		JSON.stringify(
			{
				kind: 'authored-not-driven',
				design: flowDesignHead,
				journeys: authoredFlows.map((journey) => ({
					journeyId: journey.journeyId,
					requirements: journey.requirements,
					availability: journey.availability,
					grids: journey.grids,
					keys: journey.keys,
					...(journey.inputProfile === undefined ? {} : { inputProfile: journey.inputProfile }),
					fixtures: journey.fixtures,
				})),
				admissionInputs: authoredFlows.flatMap((journey) => journey.admissionInputs),
				productPasses: 0,
				layerCHarness: {
					state: 'authored-not-driven',
					keyboardKeys: layerCKeyboardKeys,
					profiles: layerCProfiles,
					pointerJourneys: layerCPointerJourneys,
				},
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
