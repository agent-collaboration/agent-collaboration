import { expandScreenJourney, journeyGridSizes } from './journey-definition.ts';
import type { JourneyDefinition } from './journey-definition.ts';
import hard from './screens/hard.ts';
import normal from './screens/normal.ts';
import quota from './screens/quota.ts';
import sessions from './screens/sessions.ts';

export const journeyCatalogue: Readonly<Record<string, JourneyDefinition>> = Object.fromEntries(
	[hard, normal, quota, sessions].flatMap((screen) =>
		journeyGridSizes.map((grid): [string, JourneyDefinition] => {
			const journey = expandScreenJourney(screen, grid);
			return [journey.name, journey];
		}),
	),
);
