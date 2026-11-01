import { compactQuotaMarkers } from './markers/grid-markers-compact-quota.ts';
import { hardQuotaMarkers } from './markers/grid-markers-hard-quota.ts';
import { historicalMarkers } from './markers/grid-markers-historical.ts';
import { normalQuotaMarkers } from './markers/grid-markers-normal-quota.ts';
import { sessionsMarkers } from './markers/grid-markers-sessions.ts';

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
