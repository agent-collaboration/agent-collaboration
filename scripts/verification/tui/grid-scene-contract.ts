import { compactQuotaMarkers } from './grid-markers-compactQuota.ts';
import { hardQuotaMarkers } from './grid-markers-hardQuota.ts';
import { historicalMarkers } from './grid-markers-historical.ts';
import { normalQuotaMarkers } from './grid-markers-normalQuota.ts';
import { sessionsMarkers } from './grid-markers-sessions.ts';

export interface SceneMarkerContract {
	readonly required: readonly string[];
	readonly forbidden: readonly string[];
}
export const sceneMarkerContracts: Readonly<Record<string, SceneMarkerContract>> = {
	...sessionsMarkers,
	...compactQuotaMarkers,
	...normalQuotaMarkers,
	...hardQuotaMarkers,
	...historicalMarkers,
};
