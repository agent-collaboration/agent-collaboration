/** Existing read-only quota fixture's exact entry, source scenario names and release obligations. */
export const COMPONENT_TEST =
	'quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture';
export const COMPONENT_SCENARIOS = {
	normal: 'Normal',
	'large-list': 'LargeList',
	'inspection-pending': 'InspectionPending',
	'commit-pending': 'CommitPending',
	'unknown-result': 'UnknownResult',
	'save-failed': 'SaveFailed',
	'disabled-yes': 'DisabledYes',
	'revalidation-pending': 'RevalidationPending',
	'refused-result': 'RefusedResult',
	'refresh-pending': 'RefreshPending',
	'save-pending': 'SavePending',
	'visual-hard-cases': 'VisualHardCases',
} as const satisfies Readonly<Record<string, string>>;
export type ComponentScenario = keyof typeof COMPONENT_SCENARIOS;
export const PENDING_SCENARIOS: ReadonlySet<ComponentScenario> = new Set<ComponentScenario>([
	'inspection-pending',
	'commit-pending',
	'revalidation-pending',
	'refresh-pending',
	'save-pending',
]);
export function isComponentScenario(value: string): value is ComponentScenario {
	return Object.hasOwn(COMPONENT_SCENARIOS, value);
}
