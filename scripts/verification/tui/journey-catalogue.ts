import type { JourneyDefinition } from './journey-definition.ts';
import journey1 from './journeys/hard-32x24.ts';
import journey2 from './journeys/hard-40x24.ts';
import journey3 from './journeys/hard-48x12.ts';
import journey4 from './journeys/hard-48x24.ts';
import journey5 from './journeys/hard-76x30.ts';
import journey0 from './journeys/hard-120x36.ts';
import journey7 from './journeys/normal-32x24.ts';
import journey8 from './journeys/normal-40x24.ts';
import journey9 from './journeys/normal-48x12.ts';
import journey10 from './journeys/normal-48x24.ts';
import journey11 from './journeys/normal-76x30.ts';
import journey6 from './journeys/normal-120x36.ts';
import journey13 from './journeys/quota-32x24.ts';
import journey14 from './journeys/quota-40x24.ts';
import journey15 from './journeys/quota-48x12.ts';
import journey16 from './journeys/quota-48x24.ts';
import journey17 from './journeys/quota-76x30.ts';
import journey12 from './journeys/quota-120x36.ts';
import journey19 from './journeys/sessions-32x24.ts';
import journey20 from './journeys/sessions-40x24.ts';
import journey21 from './journeys/sessions-48x12.ts';
import journey22 from './journeys/sessions-48x24.ts';
import journey23 from './journeys/sessions-76x30.ts';
import journey18 from './journeys/sessions-120x36.ts';
export const journeyCatalogue: Readonly<Record<string, JourneyDefinition>> = {
	'hard-120x36': journey0,
	'hard-32x24': journey1,
	'hard-40x24': journey2,
	'hard-48x12': journey3,
	'hard-48x24': journey4,
	'hard-76x30': journey5,
	'normal-120x36': journey6,
	'normal-32x24': journey7,
	'normal-40x24': journey8,
	'normal-48x12': journey9,
	'normal-48x24': journey10,
	'normal-76x30': journey11,
	'quota-120x36': journey12,
	'quota-32x24': journey13,
	'quota-40x24': journey14,
	'quota-48x12': journey15,
	'quota-48x24': journey16,
	'quota-76x30': journey17,
	'sessions-120x36': journey18,
	'sessions-32x24': journey19,
	'sessions-40x24': journey20,
	'sessions-48x12': journey21,
	'sessions-48x24': journey22,
	'sessions-76x30': journey23,
};
