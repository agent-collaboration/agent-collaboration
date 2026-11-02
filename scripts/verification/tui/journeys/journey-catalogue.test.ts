/** Frozen pre-consolidation cases pin action bursts, checkpoint order and finish. */
import { createHash } from 'node:crypto';

import { expect, test } from 'vitest';

import { canonicalJson } from '../evidence/source-binding.ts';
import { journeyCatalogue } from './journey-catalogue.ts';

const preservedCases: Readonly<Record<string, string>> = {
	'sessions-120x36': '6804bcc5acfc7aec381d2f2535d1d1b9f0d844f4cc25f617d5dd40b931783567',
	'sessions-76x30': 'a57ecb6789f809c2ed10c408e4e213b3bf6d6fcefcae75c2f3414a40111a3944',
	'sessions-48x12': 'c44ade094c8a10d725b0a1bc2c7971fc12db6633ee72b9636f3835e50adbf4cb',
	'sessions-32x24': 'bcafc24c075e5d37c9cf7358b7ba11d6a6756fbabb1db38accb3cfdb248defd1',
	'normal-120x36': 'fbf6c1eabd4a849e10ba979b5664e6e5c4e4ef79f17e279fe960fe7042d0a9a3',
	'normal-76x30': '83f6f486f146bf5439253144a527399e20a7fefafb2ea3d14465c05485a85b0c',
	'normal-48x12': '39ac19731764848b8290ee0f46558a6b89538982b3a798acee934c2b83cd1914',
	'normal-32x24': 'b50c5068517f21bb46ae2e2e6c6fc90682ba8c8d91b393481ab98e84f2531f3b',
	'hard-120x36': '06a3d153a8ff823fa2f6acb6d1745daf82e1e9d4e4dc0dcbee28729194fedd21',
	'hard-76x30': 'f99c6bc9f1e375ff354d5cf45c00f43fc6bbdea8e3cc9800fffe00a31834c7a9',
	'hard-48x12': '1735c93b1dac2e1bae253188308b0af3c6973d7df7b6124dc67e17896f05c0cd',
	'hard-32x24': '19c9cdd8ada70c715eed3399bd5cffd272ba8b5481068e96ee48b326dd75c172',
	'quota-120x36': '2180ca36df55e1bd991da07ab407c47487ee416516541749eabb0ecdd7e1d941',
	'quota-76x30': '9033282435b0a24b17d38d5c2db92d269cd02f4c374646cdd936f01b40ccfa9d',
	'quota-48x12': '9ef4e1b8bacebf8639a2dd2a595021994a33d5575d65efb1ccc36bfd9fad6c78',
	'quota-32x24': '95a145d59d6b3ccd701fe0ccd77f7618c85e7b8d94ca723e697bd34083cf1d13',
};

test('screen journeys retain the four declared grids and frozen checkpoint/action order', () => {
	expect(Object.keys(journeyCatalogue).toSorted()).toEqual(Object.keys(preservedCases).toSorted());
	for (const [journeyName, digest] of Object.entries(preservedCases)) {
		expect(
			createHash('sha256').update(canonicalJson(journeyCatalogue[journeyName])).digest('hex'),
			journeyName,
		).toBe(digest);
	}
});
